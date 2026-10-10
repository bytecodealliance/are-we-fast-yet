//! `awfy regressions`: compare a run with its baseline. See
//! `docs/regressions.md`.

use crate::config::Config;
use crate::fingerprint::Fingerprint;
use crate::metadata::Metadata;
use crate::runs::{DataDir, RunNumber};
use anyhow::{Context, Result};
use serde::{Serialize, Serializer};
use sightglass_analysis::geomean::{self, GEOMEAN};
use sightglass_data::{EffectSize, Engine, Measurement, Phase};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The event regressions are reported on.
pub const VIRTUAL_CYCLES: &str = "virtual-cycles";

pub struct Options {
    pub run: RunNumber,
    /// Append the job summary here instead of stderr.
    pub summary: Option<PathBuf>,
    pub issue_dir: Option<PathBuf>,
    pub wasmtime_repo: Option<PathBuf>,
}

/// Why a run was not compared.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Skip {
    /// The run itself is in `excluded.toml`.
    Excluded,
    NoBaseline,
    /// What changed, from [`Fingerprint::diff`].
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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Skipped {
        run: RunNumber,
        baseline: Option<RunNumber>,
        reason: Skip,
    },
    Compared(Report),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Report {
    pub baseline: RunNumber,
    pub run: RunNumber,
    pub baseline_commit: String,
    pub commit: String,
    pub significance_level: f64,
    pub min_relative_change: f64,
    /// Virtual-cycles regressions, Geomean first, then largest first.
    pub regressions: Vec<Change>,
    /// Virtual-cycles speedups, in the same order.
    pub speedups: Vec<Change>,
    /// Other events that changed for the regressed (benchmark, phase) pairs,
    /// in the same order.
    pub other_counters: Vec<Change>,
    /// Benchmarks in only one of the runs, with a note saying which.
    pub skipped_benchmarks: Vec<(String, String)>,
}

/// A significant change of at least `min-relative-change`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Change {
    pub benchmark: String,
    #[serde(serialize_with = "display")]
    pub phase: Phase,
    pub event: String,
    pub old_mean: f64,
    pub new_mean: f64,
    /// `(new - old) / old`.
    pub relative_change: f64,
    /// `new / old` for increases and `old / new` for decreases, so always at
    /// least 1, with its confidence interval's half width.
    pub ratio: f64,
    pub ratio_ci: f64,
}

impl Change {
    pub fn is_increase(&self) -> bool {
        self.relative_change > 0.0
    }

    fn from_effect_size(e: &EffectSize<'_>) -> Self {
        let (ratio, ratio_ci) = if e.b_mean >= e.a_mean {
            e.b_speed_up_over_a()
        } else {
            e.a_speed_up_over_b()
        };
        Self {
            benchmark: e.wasm.to_string(),
            phase: e.phase,
            event: e.event.to_string(),
            old_mean: e.a_mean,
            new_mean: e.b_mean,
            relative_change: (e.b_mean - e.a_mean) / e.a_mean,
            ratio,
            ratio_ci: ratio_ci.abs(),
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

fn display<T: std::fmt::Display, S: Serializer>(t: &T, s: S) -> Result<S::Ok, S::Error> {
    s.collect_str(t)
}

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
pub fn compare_measurements(
    config: &Config,
    (baseline, old_meta, old): (RunNumber, &Metadata, Vec<Measurement<'static>>),
    (run, new_meta, new): (RunNumber, &Metadata, Vec<Measurement<'static>>),
) -> Result<Report> {
    let min = config.regressions.min_relative_change;
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

    // Every event gets a Geomean, for the other-counters section too.
    geomean::add(&mut measurements);
    let effect_sizes = sightglass_analysis::effect_size::calculate(significance, &measurements)?;

    let mut regressions = Vec::new();
    let mut speedups = Vec::new();
    let mut others = Vec::new();
    for e in &effect_sizes {
        assert_eq!(e.a_engine.name, a);
        assert_eq!(e.b_engine.name, b);
        if !e.is_significant() {
            continue;
        }
        let change = Change::from_effect_size(e);
        if change.relative_change.abs() < min {
            continue;
        }
        match (e.event == VIRTUAL_CYCLES, change.is_increase()) {
            (true, true) => regressions.push(change),
            (true, false) => speedups.push(change),
            (false, _) => others.push(change),
        }
    }
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
    speedups.sort_by(Change::order);
    other_counters.sort_by(Change::order);

    Ok(Report {
        baseline,
        run,
        baseline_commit: old_meta.wasmtime.commit.clone(),
        commit: new_meta.wasmtime.commit.clone(),
        significance_level: significance,
        min_relative_change: min,
        regressions,
        speedups,
        other_counters,
        skipped_benchmarks,
    })
}

/// The Markdown job summary.
pub fn summary(outcome: &Outcome) -> String {
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
                "Wasmtime `{}`..`{}`. A change counts when it is significant at \
                 the {} level and at least {}.\n",
                short(&r.baseline_commit),
                short(&r.commit),
                r.significance_level,
                percent(r.min_relative_change),
            )
            .unwrap();
            change_table(&mut s, "Regressions", &r.regressions);
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

fn change_table(s: &mut String, title: &str, changes: &[Change]) {
    if changes.is_empty() {
        writeln!(s, "No {}.\n", title.to_lowercase()).unwrap();
        return;
    }
    writeln!(s, "### {title} ({})\n", changes.len()).unwrap();
    writeln!(s, "| benchmark | phase | change |").unwrap();
    writeln!(s, "|-----------|-------|--------|").unwrap();
    for c in changes {
        writeln!(
            s,
            "| `{}` | {} | {} ({:.3}x +/- {:.3}) |",
            c.benchmark,
            c.phase,
            signed_percent(c.relative_change),
            c.ratio,
            c.ratio_ci
        )
        .unwrap();
    }
    writeln!(s).unwrap();
}

/// The first 12 characters of a SHA.
pub fn short(sha: &str) -> &str {
    &sha[..sha.len().min(12)]
}

fn percent(x: f64) -> String {
    format!("{}%", x * 100.0)
}

pub fn signed_percent(x: f64) -> String {
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
        compare(&DataDir::new(&fixtures()), &config(), RunNumber(run)).unwrap()
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
        assert!(compare(&DataDir::new(&fixtures()), &config(), RunNumber(99)).is_err());
    }

    #[test]
    fn fingerprint_change_skips_excluded_baseline() {
        // 00004 is excluded, so 00005's baseline is 00003.
        assert_eq!(
            skip_reason(compare_fixture(5)),
            (
                Some(RunNumber(3)),
                Skip::FingerprintChanged(vec!["valgrind 3.22.0 -> 3.24.0".into()])
            )
        );
    }

    #[test]
    fn same_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let data = DataDir::new(tmp.path());
        for run in ["00001", "00002"] {
            std::fs::create_dir_all(data.path.join(run)).unwrap();
            std::fs::copy(
                fixtures().join("benchmark-data/00001/metadata.toml"),
                data.path.join(run).join("metadata.toml"),
            )
            .unwrap();
        }
        assert_eq!(
            skip_reason(compare(&data, &config(), RunNumber(2)).unwrap()),
            (Some(RunNumber(1)), Skip::SameCommit("1".repeat(40)))
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
        // bz2 execution is +1.5%, above the 1% threshold. pulldown-cmark
        // compilation is +0.5%: significant, but below it.
        assert_eq!(
            names(&r.regressions),
            ["bz2/benchmark.wasm execution virtual-cycles"]
        );
        // The Geomean comes first. bz2 compilation is -3%, the Geomean about
        // -1.3%.
        assert_eq!(
            names(&r.speedups),
            [
                "Geomean compilation virtual-cycles",
                "bz2/benchmark.wasm compilation virtual-cycles"
            ]
        );
        assert_eq!(
            names(&r.other_counters),
            ["bz2/benchmark.wasm execution instructions-retired"]
        );
        let c = &r.regressions[0];
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

    fn measurement(engine: &str, wasm: &str, phase: Phase, count: u64) -> Measurement<'static> {
        Measurement {
            arch: "x86_64".into(),
            engine: Engine::from(engine.to_string()),
            wasm: wasm.to_string().into(),
            process: 0,
            iteration: 0,
            phase,
            event: VIRTUAL_CYCLES.into(),
            count,
        }
    }

    /// Compare runs 1 and 2 given `(wasm, phase, old counts, new counts)`.
    fn compare_counts(rows: &[(&str, Phase, &[u64], &[u64])]) -> Report {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for &(wasm, phase, a, b) in rows {
            for (i, &c) in a.iter().enumerate() {
                old.push(Measurement {
                    process: i as u32,
                    ..measurement("x", wasm, phase, c)
                });
            }
            for (i, &c) in b.iter().enumerate() {
                new.push(Measurement {
                    process: i as u32,
                    ..measurement("y", wasm, phase, c)
                });
            }
        }
        let mut meta = crate::metadata::tests::example();
        let old_meta = meta.clone();
        meta.wasmtime.commit = "f".repeat(40);
        compare_measurements(
            &config(),
            (RunNumber(1), &old_meta, old),
            (RunNumber(2), &meta, new),
        )
        .unwrap()
    }

    #[test]
    fn zero_variance() {
        // Identical samples: `behrens_fisher` errs on zero variance and
        // Sightglass falls back to a zero-width interval. An exact tie must
        // still be unchanged.
        let same = [1000, 1000, 1000];
        let r = compare_counts(&[("a.wasm", Phase::Execution, &same, &same)]);
        assert!(r.regressions.is_empty() && r.speedups.is_empty());

        // Any difference is then "significant", so the threshold decides.
        let r = compare_counts(&[
            ("a.wasm", Phase::Execution, &same, &[1005, 1005, 1005]),
            ("b.wasm", Phase::Execution, &same, &[1020, 1020, 1020]),
        ]);
        assert_eq!(r.regressions.len(), 2, "{:?}", r.regressions);
        assert_eq!(r.regressions[0].benchmark, GEOMEAN);
        assert_eq!(r.regressions[1].benchmark, "b.wasm");
    }

    #[test]
    fn insignificant() {
        // A big change in the mean that the noise swamps.
        let r = compare_counts(&[(
            "a.wasm",
            Phase::Execution,
            &[1000, 2000, 3000],
            &[1100, 2100, 3100],
        )]);
        assert!(r.regressions.is_empty());
    }

    #[test]
    fn geomean_regression_first() {
        let r = compare_counts(&[
            (
                "a.wasm",
                Phase::Compilation,
                &[100, 101, 99],
                &[150, 151, 149],
            ),
            (
                "b.wasm",
                Phase::Compilation,
                &[1000, 1001, 999],
                &[1100, 1101, 1099],
            ),
        ]);
        let order: Vec<&str> = r.regressions.iter().map(|c| c.benchmark.as_str()).collect();
        assert_eq!(order, [GEOMEAN, "a.wasm", "b.wasm"]);
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
}
