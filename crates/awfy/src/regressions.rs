//! `awfy regressions`: compare a run with its baseline. See
//! `docs/regressions.md`.

use crate::config::{Config, MinRelativeChange};
use crate::fingerprint::Fingerprint;
use crate::metadata::Metadata;
use crate::runs::{DataDir, RunNumber};
use anyhow::{Context, Result};
use serde::{Serialize, Serializer};
use sightglass_analysis::geomean::{self, GEOMEAN, GeomeanEffectSize};
use sightglass_data::{EffectSize, Engine, Measurement, Phase};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The event regressions are reported on.
pub const VIRTUAL_CYCLES: &str = "virtual-cycles";

/// The event counting work done rather than how well it was predicted.
const INSTRUCTIONS: &str = "instructions-retired";

/// The events `Suppressed` attributes to misprediction.
const BRANCH_MISS_EVENTS: [&str; 2] = ["conditional-branch-misses", "indirect-branch-misses"];

/// A branch miss's cost in virtual cycles. Keep in sync with
/// `ParsedCallgrindDump::virtual_cycles` in
/// `sightglass/crates/recorder/src/measure/callgrind.rs`.
const BRANCH_MISS_WEIGHT: f64 = 15.0;

/// Branch misses must be at least this much of the delta to suppress. Can
/// exceed 1, when every other term moved the other way.
const SUPPRESS_BRANCH_MISS_SHARE: f64 = 0.9;

/// ... and `instructions-retired` must have stayed within this much.
const SUPPRESS_INSTRUCTIONS_CHANGE: f64 = 0.001;

/// Options for the `awfy regressions` command.
#[derive(clap::Args)]
pub struct Options {
    /// The run to check, e.g. `00042` or `42`.
    #[arg(long)]
    run: RunNumber,
    /// Append the job summary to this file instead of stderr, e.g.
    /// `$GITHUB_STEP_SUMMARY`.
    #[arg(long)]
    summary: Option<PathBuf>,
    /// Write the issue title and body here, if there are regressions.
    #[arg(long)]
    issue_dir: Option<PathBuf>,
    /// A Wasmtime clone, for the issue's commit log.
    #[arg(long)]
    wasmtime_repo: Option<PathBuf>,
}

/// Why a run was not compared.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Skip {
    /// The run itself is in `excluded.toml`.
    Excluded,
    /// There is no earlier baseline to compare with.
    NoBaseline,
    /// What changed, from `Fingerprint::diff`.
    FingerprintChanged(Vec<String>),
    /// Both runs measured this Wasmtime commit, e.g. a forced re-run.
    SameCommit(String),
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::Excluded => write!(f, "the run is excluded in `excluded.toml`"),
            Skip::NoBaseline => write!(f, "there is no earlier run to compare with"),
            Skip::FingerprintChanged(changes) => {
                write!(f, "the environment changed: {}", changes.join(", "))
            }
            Skip::SameCommit(commit) => {
                write!(f, "both runs measured the same Wasmtime commit, `{commit}`")
            }
        }
    }
}

/// The result of an `awfy regressions` run.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    /// The regression comparison was skipped.
    Skipped {
        /// The number of the benchmark run that was skipped.
        run: RunNumber,
        /// The baseline benchmark run's number, if any.
        baseline: Option<RunNumber>,
        /// The reason it was skipped.
        reason: Skip,
    },
    /// The regression comparison's report.
    Compared(Report),
}

/// A report on a regression comparison between two runs.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Report {
    /// The baseline benchmark run's number.
    pub baseline: RunNumber,
    /// The benchmark run's number that was compared to the baseline.
    pub run: RunNumber,
    /// The baseline's Wasmtime commit.
    pub baseline_commit: String,
    /// The Wasmtime commit for the run being compared to baseline.
    pub commit: String,
    /// The significance level we used to test for regressions.
    pub significance_level: f64,
    /// The minimum relative changes required for reporting a regression.
    pub min_relative_change: MinRelativeChange,
    /// Virtual-cycles regressions, Geomean first, then largest first.
    pub regressions: Vec<Change>,
    /// Regressions held back by the branch-aliasing rule, in the same order.
    pub suppressed: Vec<Suppressed>,
    /// Virtual-cycles speedups, in the same order.
    speedups: Vec<Change>,
    /// Other events that changed for the regressed (benchmark, phase) pairs,
    /// in the same order.
    pub other_counters: Vec<Change>,
    /// Benchmarks in only one of the runs, with a note saying which.
    skipped_benchmarks: Vec<(String, String)>,
}

/// A significant change of at least `min-relative-change`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Change {
    /// The Wasm benchmark that changed.
    pub benchmark: String,
    /// The phase that changed.
    #[serde(serialize_with = "display")]
    pub phase: Phase,
    /// The event that changed.
    pub event: String,
    /// The baseline run's median count, or its geomean for [`GEOMEAN`].
    pub old: f64,
    /// The new run's median count, or its geomean for [`GEOMEAN`].
    pub new: f64,
    /// How much bigger the new run's counts are, signed: `+0.015` is 1.5%
    /// more. This is the tested ratio rather than a difference of the two
    /// counts above, so that `min-relative-change` filters on the same
    /// quantity the significance test bounds.
    pub relative_change: f64,
    /// `new / old` for increases and `old / new` for decreases, so always at
    /// least 1.
    pub ratio: f64,
    /// The low end of `ratio`'s confidence interval. The interval is not
    /// symmetric about `ratio`, so both ends are kept.
    pub ratio_low: f64,
    /// The high end of `ratio`'s confidence interval.
    pub ratio_high: f64,
    /// How far the baseline's counts spread, as a percentage of its median.
    /// `None` for [`GEOMEAN`], which aggregates means and has no samples of
    /// its own to spread.
    pub old_noise: Option<f64>,
    /// The same for the new run. See `old_noise`.
    pub new_noise: Option<f64>,
}

impl Change {
    /// Is this change an increase or decrease?
    pub fn is_increase(&self) -> bool {
        self.relative_change > 0.0
    }

    /// Engine `a` must be named `baseline` and engine `b` named `run`.
    ///
    /// Only call this for a significant `e`, which always has an interval.
    fn from_effect_size(e: &EffectSize<'_>, baseline: &str, run: &str) -> Self {
        assert_eq!(e.a_engine.name, baseline);
        assert_eq!(e.b_engine.name, run);
        // `e.ratio` is the new run's counts over the baseline's, so the
        // baseline is the faster engine when it exceeds 1.
        let (ratio, interval) = if e.ratio >= 1.0 {
            e.b_speed_up_over_a()
        } else {
            e.a_speed_up_over_b()
        };
        let (ratio_low, ratio_high) = interval.expect("a significant change has an interval");
        Self {
            benchmark: e.wasm.to_string(),
            phase: e.phase,
            event: e.event.to_string(),
            old: e.a_median,
            new: e.b_median,
            relative_change: e.ratio - 1.0,
            ratio,
            ratio_low,
            ratio_high,
            old_noise: Some(e.a_noise()),
            new_noise: Some(e.b_noise()),
        }
    }

    /// The same for a suite geomean, which is compared as a whole rather than
    /// as a benchmark. Engine `a` must be named `baseline` and `b` named `run`.
    ///
    /// Only call this for a significant `e`, which always has an interval.
    fn from_geomean(e: &GeomeanEffectSize<'_>, baseline: &str, run: &str) -> Self {
        assert_eq!(e.a_engine.name, baseline);
        assert_eq!(e.b_engine.name, run);
        let (low, high, _dof) = e
            .confidence_interval
            .expect("a significant change has an interval");
        // As in `from_effect_size`, except that inverting the ratio here means
        // inverting the interval by hand: a `GeomeanEffectSize` has no
        // `a_speed_up_over_b`.
        let (ratio, ratio_low, ratio_high) = if e.ratio >= 1.0 {
            (e.ratio, low, high)
        } else {
            (1.0 / e.ratio, 1.0 / high, 1.0 / low)
        };
        Self {
            benchmark: GEOMEAN.to_string(),
            phase: e.phase,
            event: e.event.to_string(),
            old: e.a_geomean,
            new: e.b_geomean,
            relative_change: e.ratio - 1.0,
            ratio,
            ratio_low,
            ratio_high,
            old_noise: None,
            new_noise: None,
        }
    }

    /// Geomean first, then larger ratios, then by name for stability.
    fn order(a: &Self, b: &Self) -> std::cmp::Ordering {
        (b.benchmark == GEOMEAN)
            .cmp(&(a.benchmark == GEOMEAN))
            .then(b.ratio.total_cmp(&a.ratio))
            .then_with(|| (&a.benchmark, a.phase, &a.event).cmp(&(&b.benchmark, b.phase, &b.event)))
    }
}

/// Every event's `(old, new)` medians, keyed by benchmark, phase, and event.
///
/// Includes counters that moved too little to be reported in their own right,
/// which is what `Suppressed::check` needs.
type Medians<'a> = BTreeMap<(&'a str, Phase, &'a str), (f64, f64)>;

/// A regression held back because its delta is branch misses alone, with
/// `instructions-retired` flat.
///
/// Callgrind's branch predictors are small and aliasing-prone, so a code
/// layout change can collide two hot branches and explode the miss count with
/// no change in the work done. See `docs/regressions.md`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Suppressed {
    /// The regression that would otherwise have been reported.
    pub change: Change,
    /// Branch misses' share of the virtual-cycles delta, as a fraction. Can
    /// exceed 1, when every other term moved the other way.
    pub branch_miss_share: f64,
    /// How much `instructions-retired` changed, signed, as a fraction.
    pub instructions_change: f64,
}

impl Suppressed {
    /// Hold `c` back if it carries the signature. Only execution-phase
    /// benchmarks are candidates; the geomean has no counters of its own.
    fn check(c: &Change, medians: &Medians) -> Option<Self> {
        if c.phase != Phase::Execution || c.benchmark == GEOMEAN {
            return None;
        }
        let median = |event| {
            medians
                .get(&(c.benchmark.as_str(), c.phase, event))
                .copied()
        };
        let delta = c.new - c.old;
        if delta <= 0.0 {
            return None;
        }
        let mut branch_miss_delta = 0.0;
        for event in BRANCH_MISS_EVENTS {
            let (old, new) = median(event)?;
            branch_miss_delta += BRANCH_MISS_WEIGHT * (new - old);
        }
        let (old, new) = median(INSTRUCTIONS)?;
        if old <= 0.0 {
            return None;
        }
        let branch_miss_share = branch_miss_delta / delta;
        let instructions_change = (new - old) / old;
        let matches = branch_miss_share >= SUPPRESS_BRANCH_MISS_SHARE
            && instructions_change.abs() < SUPPRESS_INSTRUCTIONS_CHANGE;
        matches.then(|| Self {
            change: c.clone(),
            branch_miss_share,
            instructions_change,
        })
    }
}

/// Serialize with `Display`, e.g. for a `Timestamp` in JSON.
pub(crate) fn display<T: std::fmt::Display, S: Serializer>(t: &T, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(t)
}

/// Run `awfy regressions` with the given options.
pub fn run(root: &Path, options: Options) -> Result<()> {
    let config = Config::load(root)?;
    let data = DataDir::new(root);
    let outcome = compare(&data, &config, options.run)?;

    let summary = summary(&outcome);
    match &options.summary {
        Some(path) => {
            use std::io::Write;
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut f| f.write_all(summary.as_bytes()))
                .with_context(|| format!("appending to {}", path.display()))?;
        }
        None => eprint!("{summary}"),
    }
    if let (Some(dir), Outcome::Compared(report)) = (&options.issue_dir, &outcome) {
        let metadata = data.metadata(report.run)?;
        let wrote = crate::issue::write(
            dir,
            report,
            &metadata,
            &config,
            options.wasmtime_repo.as_deref(),
        )?;
        if wrote {
            log::info!("wrote issue to {}", dir.display());
        }
    }
    Ok(())
}

/// Find `run`'s baseline and compare the two.
pub fn compare(data: &DataDir, config: &Config, run: RunNumber) -> Result<Outcome> {
    let visible = data.visible_runs()?;
    let skipped = |baseline, reason| {
        Ok(Outcome::Skipped {
            run,
            baseline,
            reason,
        })
    };
    if !data.runs()?.contains(&run) {
        anyhow::bail!("run {run} does not exist");
    }
    if !visible.contains(&run) {
        return skipped(None, Skip::Excluded);
    }
    let Some(&baseline) = visible.iter().rev().find(|&&r| r < run) else {
        return skipped(None, Skip::NoBaseline);
    };

    let old = data.metadata(baseline)?;
    let new = data.metadata(run)?;
    let changes = Fingerprint::of(&old).diff(&Fingerprint::of(&new));
    if !changes.is_empty() {
        return skipped(Some(baseline), Skip::FingerprintChanged(changes));
    }
    if old.wasmtime.commit == new.wasmtime.commit {
        return skipped(Some(baseline), Skip::SameCommit(new.wasmtime.commit));
    }

    let report = compare_measurements(
        config,
        (baseline, &old, data.measurements(baseline)?),
        (run, &new, data.measurements(run)?),
    )?;
    Ok(Outcome::Compared(report))
}

/// The comparison itself, once the baseline has been chosen.
fn compare_measurements(
    config: &Config,
    (baseline, old_meta, old): (RunNumber, &Metadata, Vec<Measurement<'static>>),
    (run, new_meta, new): (RunNumber, &Metadata, Vec<Measurement<'static>>),
) -> Result<Report> {
    let min = &config.regressions.min_relative_change;
    let significance = config.regressions.significance_level;

    // Align: compare only benchmarks both runs measured.
    let benchmarks = |ms: &[Measurement]| -> BTreeSet<String> {
        ms.iter().map(|m| m.wasm.to_string()).collect()
    };
    let (old_benchmarks, new_benchmarks) = (benchmarks(&old), benchmarks(&new));
    let mut skipped_benchmarks = Vec::new();
    for b in old_benchmarks.difference(&new_benchmarks) {
        skipped_benchmarks.push((b.clone(), format!("not in run {run}")));
    }
    for b in new_benchmarks.difference(&old_benchmarks) {
        skipped_benchmarks.push((b.clone(), format!("not in run {baseline}")));
    }
    skipped_benchmarks.sort();
    let shared: BTreeSet<&String> = old_benchmarks.intersection(&new_benchmarks).collect();

    // Name each engine after its run, zero-padded so that sorted engine
    // order (which `effect_size::calculate` uses to pick `a` and `b`) is run
    // order.
    let (a, b) = (baseline.to_string(), run.to_string());
    assert!(a < b, "baseline {a} must sort before run {b}");
    let mut measurements: Vec<Measurement<'static>> = Vec::with_capacity(old.len() + new.len());
    for (name, ms) in [(&a, old), (&b, new)] {
        measurements.extend(
            ms.into_iter()
                .filter(|m| shared.contains(&m.wasm.to_string()))
                .map(|mut m| {
                    m.engine = Engine::from(name.clone());
                    m
                }),
        );
    }

    // The suite geomean is compared as a whole rather than as a synthetic
    // benchmark, so it is a second pass over the same measurements. Every
    // event gets one, for the other-counters section too.
    let effect_sizes = sightglass_analysis::effect_size::calculate(significance, &measurements)?;
    let geomeans = geomean::effect_size(significance, &measurements);

    let mut regressions = Vec::new();
    let mut speedups = Vec::new();
    let mut others = Vec::new();
    let mut classify = |event: &str, change: Change| {
        let thresholds = min.for_phase(change.phase);
        let threshold = if change.benchmark == GEOMEAN {
            thresholds.geomean
        } else {
            thresholds.individual
        };
        if change.relative_change.abs() < threshold {
            return;
        }
        match (event == VIRTUAL_CYCLES, change.is_increase()) {
            (true, true) => regressions.push(change),
            (true, false) => speedups.push(change),
            (false, _) => others.push(change),
        }
    };
    for e in &geomeans {
        if e.is_significant() {
            classify(&e.event, Change::from_geomean(e, &a, &b));
        }
    }
    for e in &effect_sizes {
        if e.is_significant() {
            classify(&e.event, Change::from_effect_size(e, &a, &b));
        }
    }
    // Every event, significant or not: the counters `Suppressed::check` reads
    // often move too little to be classified above.
    let medians: Medians = effect_sizes
        .iter()
        .map(|e| {
            (
                (e.wasm.as_ref(), e.phase, e.event.as_ref()),
                (e.a_median, e.b_median),
            )
        })
        .collect();
    let mut suppressed = Vec::new();
    regressions.retain(|c| match Suppressed::check(c, &medians) {
        Some(s) => {
            suppressed.push(s);
            false
        }
        None => true,
    });

    let regressed: BTreeSet<(&str, Phase)> = regressions
        .iter()
        .map(|c| (c.benchmark.as_str(), c.phase))
        .collect();
    let mut other_counters: Vec<Change> = others
        .iter()
        .filter(|c| regressed.contains(&(c.benchmark.as_str(), c.phase)))
        .cloned()
        .collect();

    regressions.sort_by(Change::order);
    suppressed.sort_by(|a, b| Change::order(&a.change, &b.change));
    speedups.sort_by(Change::order);
    other_counters.sort_by(Change::order);

    Ok(Report {
        baseline,
        run,
        baseline_commit: old_meta.wasmtime.commit.clone(),
        commit: new_meta.wasmtime.commit.clone(),
        significance_level: significance,
        min_relative_change: min.clone(),
        regressions,
        suppressed,
        speedups,
        other_counters,
        skipped_benchmarks,
    })
}

/// The Markdown job summary.
fn summary(outcome: &Outcome) -> String {
    let mut s = String::new();
    match outcome {
        Outcome::Skipped {
            run,
            baseline,
            reason,
        } => {
            match baseline {
                Some(baseline) => writeln!(s, "## Regressions: run {run} vs. {baseline}\n"),
                None => writeln!(s, "## Regressions: run {run}\n"),
            }
            .unwrap();
            writeln!(s, "Not compared: {reason}.\n").unwrap();
        }
        Outcome::Compared(r) => {
            writeln!(s, "## Regressions: run {} vs. {}\n", r.run, r.baseline).unwrap();
            writeln!(
                s,
                "Wasmtime `{}`..`{}`.\n\n\
                 A change counts when a Mann-Whitney U test finds it \
                 significant at the `{}` level and it clears its phase's \
                 minimum relative change:\n",
                short(&r.baseline_commit),
                short(&r.commit),
                r.significance_level,
            )
            .unwrap();
            threshold_table(&mut s, &r.min_relative_change);
            change_table(&mut s, "Regressions", &r.regressions);
            suppressed_table(&mut s, &r.suppressed);
            change_table(&mut s, "Speedups", &r.speedups);
            if !r.skipped_benchmarks.is_empty() {
                writeln!(s, "### Skipped benchmarks\n").unwrap();
                for (b, why) in &r.skipped_benchmarks {
                    writeln!(s, "* `{b}`: {why}").unwrap();
                }
                writeln!(s).unwrap();
            }
        }
    }
    s
}

/// The thresholds a change had to clear, as a Markdown table.
pub fn threshold_table(s: &mut String, min: &MinRelativeChange) {
    writeln!(s, "| phase | geomean | individual |").unwrap();
    writeln!(s, "|-------|---------|------------|").unwrap();
    for (phase, t) in min.phases() {
        writeln!(
            s,
            "| {phase} | {} | {} |",
            percent(t.geomean),
            percent(t.individual)
        )
        .unwrap();
    }
    writeln!(s).unwrap();
}

/// The job summary is the only place suppressions are visible, so each one
/// shows the numbers that justify it.
fn suppressed_table(s: &mut String, suppressed: &[Suppressed]) {
    if suppressed.is_empty() {
        return;
    }
    writeln!(s, "### Suppressed ({})\n", suppressed.len()).unwrap();
    writeln!(
        s,
        "Virtual cycles moved, but the delta is branch misses alone and the \
         instruction count held still. That is Valgrind's branch predictors \
         aliasing on a code layout change, not a regression; see \
         `docs/regressions.md`.\n"
    )
    .unwrap();
    writeln!(
        s,
        "| benchmark | phase | change | branch-miss share | instructions |"
    )
    .unwrap();
    writeln!(
        s,
        "|-----------|-------|--------|-------------------|--------------|"
    )
    .unwrap();
    for x in suppressed {
        writeln!(
            s,
            "| `{}` | {} | {} ({}) | {:.1}% | {} |",
            x.change.benchmark,
            x.change.phase,
            signed_percent(x.change.relative_change),
            interval(&x.change),
            x.branch_miss_share * 100.0,
            signed_percent(x.instructions_change),
        )
        .unwrap();
    }
    writeln!(s).unwrap();
}

fn change_table(s: &mut String, title: &str, changes: &[Change]) {
    if changes.is_empty() {
        writeln!(s, "### {title}\n").unwrap();
        writeln!(s, "No {}.\n", title.to_lowercase()).unwrap();
        return;
    }
    writeln!(s, "### {title} ({})\n", changes.len()).unwrap();
    writeln!(s, "| benchmark | phase | change | noise |").unwrap();
    writeln!(s, "|-----------|-------|--------|-------|").unwrap();
    for c in changes {
        writeln!(
            s,
            "| `{}` | {} | {} ({}) | {} |",
            c.benchmark,
            c.phase,
            signed_percent(c.relative_change),
            interval(c),
            noise(c),
        )
        .unwrap();
    }
    writeln!(s).unwrap();
}

/// A change's ratio and its confidence interval, e.g.
/// `1.015x, 1.010x to 1.020x`.
pub fn interval(c: &Change) -> String {
    format!(
        "{:.3}x, {:.3}x to {:.3}x",
        c.ratio, c.ratio_low, c.ratio_high
    )
}

/// How far each run's counts spread, e.g. `0.012% -> 0.015%`. The suite
/// geomean aggregates means rather than samples, so it has no spread.
pub fn noise(c: &Change) -> String {
    match (c.old_noise, c.new_noise) {
        (Some(old), Some(new)) => format!("{old:.3}% -> {new:.3}%"),
        _ => "n/a".to_string(),
    }
}

/// The first 12 characters of a SHA.
pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn percent(x: f64) -> String {
    format!("{}%", x * 100.0)
}

/// Get a string describing the given ratio `x` as a signed percent.
fn signed_percent(x: f64) -> String {
    format!("{:+.2}%", x * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::{assert_snapshot, json};

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn config() -> Config {
        Config::load(&fixtures()).unwrap()
    }

    fn compare_fixture(run: u32) -> Outcome {
        compare(
            &DataDir::new(&fixtures()),
            &config(),
            RunNumber::new(run).unwrap(),
        )
        .unwrap()
    }

    /// The JSON with floats at 6 significant digits, so last-bit differences
    /// between platforms' math libraries don't break snapshots.
    fn stable_json(outcome: &Outcome) -> String {
        fn round(v: &mut serde_json::Value) {
            match v {
                serde_json::Value::Number(n) if n.is_f64() => {
                    let f = n.as_f64().unwrap();
                    let rounded: f64 = format!("{f:.5e}").parse().unwrap();
                    *v = serde_json::json!(rounded);
                }
                serde_json::Value::Array(a) => a.iter_mut().for_each(round),
                serde_json::Value::Object(o) => o.values_mut().for_each(round),
                _ => {}
            }
        }
        let mut v = serde_json::to_value(outcome).unwrap();
        round(&mut v);
        json(&v)
    }

    fn report(outcome: Outcome) -> Report {
        match outcome {
            Outcome::Compared(r) => r,
            Outcome::Skipped { reason, .. } => panic!("skipped: {reason}"),
        }
    }

    fn skip_reason(outcome: Outcome) -> (Option<RunNumber>, Skip) {
        match outcome {
            Outcome::Skipped {
                baseline, reason, ..
            } => (baseline, reason),
            Outcome::Compared(_) => panic!("compared"),
        }
    }

    #[test]
    fn no_baseline() {
        assert_eq!(skip_reason(compare_fixture(1)), (None, Skip::NoBaseline));
    }

    #[test]
    fn excluded_run() {
        assert_eq!(skip_reason(compare_fixture(4)), (None, Skip::Excluded));
    }

    #[test]
    fn missing_run() {
        assert!(
            compare(
                &DataDir::new(&fixtures()),
                &config(),
                RunNumber::new(99).unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn fingerprint_change_skips_excluded_baseline() {
        // 00004 is excluded, so 00005's baseline is 00003.
        assert_eq!(
            skip_reason(compare_fixture(5)),
            (
                Some(RunNumber::new(3).unwrap()),
                Skip::FingerprintChanged(vec!["valgrind 3.22.0 -> 3.24.0".into()])
            )
        );
    }

    #[test]
    fn same_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let data = DataDir::new(tmp.path());
        for run in ["00001", "00002"] {
            std::fs::create_dir_all(data.path().join(run)).unwrap();
            std::fs::copy(
                fixtures().join("benchmark-data/00001/metadata.toml"),
                data.path().join(run).join("metadata.toml"),
            )
            .unwrap();
        }
        assert_eq!(
            skip_reason(compare(&data, &config(), RunNumber::new(2).unwrap()).unwrap()),
            (
                Some(RunNumber::new(1).unwrap()),
                Skip::SameCommit("1".repeat(40))
            )
        );
    }

    #[test]
    fn steady() {
        let r = report(compare_fixture(2));
        assert!(r.regressions.is_empty());
        assert!(r.speedups.is_empty());
        assert!(r.other_counters.is_empty());
    }

    #[test]
    fn threshold() {
        let r = report(compare_fixture(3));
        let names = |cs: &[Change]| -> Vec<String> {
            cs.iter()
                .map(|c| format!("{} {} {}", c.benchmark, c.phase, c.event))
                .collect()
        };
        // bz2 execution is +1.5%, above execution's 1% individual threshold.
        // pulldown-cmark compilation is +0.5%: significant, but below
        // compilation's. The execution Geomean comes first, and clears that
        // phase's much tighter 0.2% geomean threshold.
        assert_eq!(
            names(&r.regressions),
            [
                "Geomean execution virtual-cycles",
                "bz2/benchmark.wasm execution virtual-cycles"
            ]
        );
        // bz2 compilation is -3%, above compilation's 1% individual threshold.
        // The compilation Geomean is about -1.3%, below that phase's 2%
        // geomean threshold, so only the benchmark is reported.
        assert_eq!(
            names(&r.speedups),
            ["bz2/benchmark.wasm compilation virtual-cycles"]
        );
        // Both execution regressions carry their other counters along.
        assert_eq!(
            names(&r.other_counters),
            [
                "Geomean execution instructions-retired",
                "bz2/benchmark.wasm execution instructions-retired"
            ]
        );
        let c = &r.regressions[1];
        assert!((c.relative_change - 0.015).abs() < 1e-9);
        assert!((c.ratio - 1.015).abs() < 1e-9);
        assert!(r.skipped_benchmarks.is_empty());
    }

    #[test]
    fn failed_benchmark() {
        // 00006 lacks pulldown-cmark. With one benchmark left, there is no
        // Geomean.
        let r = report(compare_fixture(6));
        assert_eq!(
            r.skipped_benchmarks,
            [(
                "pulldown-cmark/benchmark.wasm".to_string(),
                "not in run 00006".to_string()
            )]
        );
        assert!(r.regressions.is_empty());
        assert!(r.speedups.is_empty());
    }

    fn measurement(
        engine: &str,
        wasm: &str,
        phase: Phase,
        event: &str,
        count: u64,
    ) -> Measurement<'static> {
        Measurement {
            arch: "x86_64".into(),
            engine: Engine::from(engine.to_string()),
            wasm: wasm.to_string().into(),
            phase,
            event: event.to_string().into(),
            count,
        }
    }

    /// One `(wasm, phase, event)`'s counts in each of the two runs.
    type Row<'a> = (&'a str, Phase, &'a str, &'a [u64], &'a [u64]);

    /// A `Row` owning its counts.
    type OwnedRow = (&'static str, Phase, &'static str, Vec<u64>, Vec<u64>);

    /// Compare runs 1 and 2 given each `(wasm, phase)`'s virtual cycles.
    fn compare_counts(rows: &[(&str, Phase, &[u64], &[u64])]) -> Report {
        let rows: Vec<Row> = rows
            .iter()
            .map(|&(wasm, phase, a, b)| (wasm, phase, VIRTUAL_CYCLES, a, b))
            .collect();
        compare_events(&rows)
    }

    /// The same, with the event named, for the other counters
    /// `Suppressed::check` reads.
    fn compare_events(rows: &[Row]) -> Report {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for &(wasm, phase, event, a, b) in rows {
            for &c in a {
                old.push(measurement("x", wasm, phase, event, c));
            }
            for &c in b {
                new.push(measurement("y", wasm, phase, event, c));
            }
        }
        let mut meta = crate::metadata::tests::example();
        let old_meta = meta.clone();
        meta.wasmtime.commit = "f".repeat(40);
        compare_measurements(
            &config(),
            (RunNumber::new(1).unwrap(), &old_meta, old),
            (RunNumber::new(2).unwrap(), &meta, new),
        )
        .unwrap()
    }

    #[test]
    fn zero_variance() {
        // An exact tie is every sample tied, so the U test cannot rank the two
        // runs apart at all and reports no difference.
        let same = [1000; 6];
        let r = compare_counts(&[("a.wasm", Phase::Execution, &same, &same)]);
        assert!(r.regressions.is_empty() && r.speedups.is_empty());

        // Any difference separates the samples completely, which is as
        // significant as six of each can be, so the threshold decides.
        let r = compare_counts(&[
            ("a.wasm", Phase::Execution, &same, &[1005; 6]),
            ("b.wasm", Phase::Execution, &same, &[1020; 6]),
        ]);
        let names: Vec<&str> = r.regressions.iter().map(|c| c.benchmark.as_str()).collect();
        // `a.wasm` moved 0.5%, under execution's 1% individual threshold.
        //
        // The Geomean is absent for a different reason: it is compared by a
        // delta-method interval built from each benchmark's variance, and
        // these samples have none, so it has no interval and is never
        // significant. Deterministic counts in the real data always vary a
        // little between processes.
        assert_eq!(names, ["b.wasm"], "{:?}", r.regressions);
    }

    #[test]
    fn insignificant() {
        // A big change in the median that the spread swamps: the two runs'
        // samples interleave, so no ranking separates them.
        let r = compare_counts(&[(
            "a.wasm",
            Phase::Execution,
            &[1000, 2000, 3000, 4000, 5000, 6000],
            &[1100, 2100, 3100, 4100, 5100, 6100],
        )]);
        assert!(r.regressions.is_empty(), "{:?}", r.regressions);
    }

    /// The same change in two phases, between the fixture's 1% compilation
    /// and 5% instantiation individual thresholds.
    #[test]
    fn per_phase_thresholds() {
        let old = [1000, 1001, 999, 1002, 998, 1000];
        let new = [1030, 1031, 1029, 1032, 1028, 1030];
        // One benchmark, so there is no Geomean to report alongside it.
        let r = compare_counts(&[
            ("a.wasm", Phase::Compilation, &old, &new),
            ("a.wasm", Phase::Instantiation, &old, &new),
        ]);
        let phases: Vec<Phase> = r.regressions.iter().map(|c| c.phase).collect();
        assert_eq!(phases, [Phase::Compilation], "{:?}", r.regressions);
    }

    #[test]
    fn geomean_regression_first() {
        let r = compare_counts(&[
            (
                "a.wasm",
                Phase::Compilation,
                &[100, 101, 99, 102, 98, 100],
                &[150, 151, 149, 152, 148, 150],
            ),
            (
                "b.wasm",
                Phase::Compilation,
                &[1000, 1001, 999, 1002, 998, 1000],
                &[1100, 1101, 1099, 1102, 1098, 1100],
            ),
        ]);
        let order: Vec<&str> = r.regressions.iter().map(|c| c.benchmark.as_str()).collect();
        assert_eq!(order, [GEOMEAN, "a.wasm", "b.wasm"]);
    }

    /// A benchmark whose counts rose by the given deltas. The spread makes
    /// the two runs separate completely, so the comparison is significant.
    fn branch_miss_rows(vcycles_delta: i64, miss_delta: i64, insts_delta: i64) -> Vec<OwnedRow> {
        let spread = |base: i64| -> Vec<u64> {
            [-2i64, -1, 0, 0, 1, 2]
                .iter()
                .map(|d| (base + d) as u64)
                .collect()
        };
        vec![
            (
                "a.wasm",
                Phase::Execution,
                VIRTUAL_CYCLES,
                spread(1_000_000),
                spread(1_000_000 + vcycles_delta),
            ),
            (
                "a.wasm",
                Phase::Execution,
                INSTRUCTIONS,
                spread(800_000),
                spread(800_000 + insts_delta),
            ),
            (
                "a.wasm",
                Phase::Execution,
                "conditional-branch-misses",
                spread(10_000),
                spread(10_000 + miss_delta),
            ),
            (
                "a.wasm",
                Phase::Execution,
                "indirect-branch-misses",
                spread(10_000),
                spread(10_000),
            ),
        ]
    }

    fn compare_branch_misses(vcycles_delta: i64, miss_delta: i64, insts_delta: i64) -> Report {
        let rows = branch_miss_rows(vcycles_delta, miss_delta, insts_delta);
        let rows: Vec<_> = rows
            .iter()
            .map(|(w, p, e, a, b)| (*w, *p, *e, a.as_slice(), b.as_slice()))
            .collect();
        compare_events(&rows)
    }

    /// Virtual cycles up 4%, all of it branch misses, instructions flat.
    #[test]
    fn suppresses_branch_aliasing() {
        // 40000 virtual cycles is 2667 misses at weight 15.
        let r = compare_branch_misses(40_000, 2_667, 0);
        assert!(r.regressions.is_empty(), "{:?}", r.regressions);
        assert_eq!(r.suppressed.len(), 1, "{:?}", r.suppressed);
        let x = &r.suppressed[0];
        assert_eq!(x.change.benchmark, "a.wasm");
        assert!((x.branch_miss_share - 1.0).abs() < 0.01, "{x:?}");
        assert_eq!(x.instructions_change, 0.0);
        // Not a regressed pair, so it carries no other counters either.
        assert!(r.other_counters.is_empty(), "{:?}", r.other_counters);
    }

    /// Instructions moved with it: real work, so it stays reportable however
    /// much of the delta the misses explain.
    #[test]
    fn instructions_moving_defeats_suppression() {
        let r = compare_branch_misses(40_000, 2_667, 4_000);
        assert_eq!(r.suppressed, []);
        assert_eq!(r.regressions.len(), 1, "{:?}", r.regressions);
    }

    /// Branch misses explain only part of the delta, so the rest is something
    /// else.
    #[test]
    fn partial_branch_miss_share_is_reported() {
        let r = compare_branch_misses(40_000, 1_333, 0);
        assert_eq!(r.suppressed, []);
        assert_eq!(r.regressions.len(), 1, "{:?}", r.regressions);
    }

    /// A run that recorded only virtual cycles must not be trusted into
    /// suppression.
    #[test]
    fn missing_counters_are_not_suppressed() {
        let r = compare_counts(&[(
            "a.wasm",
            Phase::Execution,
            &[1000, 1001, 999, 1002, 998, 1000],
            &[1040, 1041, 1039, 1042, 1038, 1040],
        )]);
        assert_eq!(r.suppressed, []);
        assert_eq!(r.regressions.len(), 1, "{:?}", r.regressions);
    }

    /// The same signature in another phase is still reported.
    #[test]
    fn other_phases_are_not_suppressed() {
        let rows = branch_miss_rows(40_000, 2_667, 0);
        let rows: Vec<_> = rows
            .iter()
            .map(|(w, _, e, a, b)| (*w, Phase::Compilation, *e, a.as_slice(), b.as_slice()))
            .collect();
        let r = compare_events(&rows);
        assert_eq!(r.suppressed, []);
        assert_eq!(r.regressions.len(), 1, "{:?}", r.regressions);
    }

    #[test]
    fn snapshots() {
        for run in 1..=6 {
            let outcome = compare_fixture(run);
            assert_snapshot(
                &format!("regressions/{run:05}.json"),
                &stable_json(&outcome),
            );
            assert_snapshot(&format!("regressions/{run:05}.md"), &summary(&outcome));
        }
    }

    /// The fixture runs have no branch-miss counters, so the suppressed
    /// section needs its own snapshot.
    #[test]
    fn suppressed_summary_snapshot() {
        let outcome = Outcome::Compared(compare_branch_misses(40_000, 2_667, 0));
        assert_snapshot("regressions/suppressed.md", &summary(&outcome));
    }
}
