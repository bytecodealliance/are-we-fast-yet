//! `awfy bisect-plan` and `awfy bisect-report`: find which commits in a
//! regressed run's range caused the regression, before filing it. See
//! `docs/regressions.md` ("Bisect").
//!
//! The plan lists every first-parent commit in the range as a candidate. Each
//! candidate is measured beside its parent on one runner, in just the
//! regressed (benchmark, phase) pairs. The report then compares each
//! candidate's own change with the change the main run detected.
//!
//! A range of one commit needs no measuring: the report blames it outright.

use crate::config::Config;
use crate::git::{Candidate, FirstParents, GitLog, Repo};
use crate::issue::Bisection;
use crate::markdown::{self, short, signed_percent};
use crate::regressions::{self, Outcome, Report, VIRTUAL_CYCLES};
use crate::run::Pair;
use crate::runs::{DataDir, RunNumber};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sightglass_analysis::geomean::{self, GEOMEAN};
use sightglass_data::{Measurement, Phase};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The plan's file name in `bisect-plan --out`.
pub const PLAN: &str = "plan.json";

/// The most candidates one bisect measures: GitHub's limit on a matrix.
const MAX_CANDIDATES: usize = 256;

/// Options for the `awfy bisect-plan` command.
#[derive(clap::Args)]
pub struct PlanOptions {
    /// The regressed run, e.g. `00042` or `42`.
    #[arg(long)]
    run: RunNumber,
    /// A Wasmtime clone holding the run's range.
    #[arg(long)]
    wasmtime_repo: PathBuf,
    /// Directory to write `plan.json` to.
    #[arg(long)]
    out: PathBuf,
}

/// Options for the `awfy bisect-report` command.
#[derive(clap::Args)]
pub struct ReportOptions {
    /// The regressed run, e.g. `00042` or `42`.
    #[arg(long)]
    run: RunNumber,
    /// The plan `bisect-plan` wrote. Without one, the bisect failed before
    /// measuring anything.
    #[arg(long)]
    plan: Option<PathBuf>,
    /// Holds a `bisect-<commit>/{parent,candidate}/` scratch pair per measured
    /// candidate. A missing pair leaves that candidate unmeasured.
    #[arg(long)]
    results: PathBuf,
    /// Append the job summary to this file instead of stderr.
    #[arg(long)]
    summary: Option<PathBuf>,
    /// Write the issue title and body here, if the bisect confirms a
    /// regression or could not check it.
    #[arg(long)]
    issue_dir: Option<PathBuf>,
    /// A Wasmtime clone, for the issue's commit log.
    #[arg(long)]
    wasmtime_repo: Option<PathBuf>,
    /// URL of the bisect's workflow run, linked from the issue.
    #[arg(long)]
    workflow_run: Option<String>,
}

/// A regressed (benchmark, phase), where the benchmark may be [`GEOMEAN`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Regressed {
    /// The benchmark, or [`GEOMEAN`].
    pub benchmark: String,
    /// The phase.
    pub phase: Phase,
}

/// What a bisect measures. Written by `bisect-plan`, read by `bisect-report`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Plan {
    /// The regressed run.
    pub run: RunNumber,
    /// The regressions to attribute.
    pub regressed: Vec<Regressed>,
    /// The (phase, benchmark) pairs each candidate is measured in, as
    /// `awfy run --pair` takes them.
    pub pairs: Vec<String>,
    /// Every first-parent commit in the run's range, newest first.
    pub candidates: Vec<Candidate>,
}

impl Plan {
    /// Whether the candidates need measuring. A range of one commit doesn't:
    /// that commit is responsible for everything the main run detected.
    pub fn measures(&self) -> bool {
        self.candidates.len() > 1
    }

    /// The GitHub Actions matrix: one job per candidate, or none if the plan
    /// [measures](Self::measures) nothing.
    pub fn matrix(&self) -> serde_json::Value {
        let pairs = self.pairs.join(" ");
        let candidates = if self.measures() {
            &self.candidates[..]
        } else {
            &[]
        };
        let include: Vec<serde_json::Value> = candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "commit": c.commit,
                    "parent": c.parent,
                    "pairs": pairs,
                })
            })
            .collect();
        serde_json::json!({ "include": include })
    }
}

/// Run `awfy bisect-plan`: write `plan.json` and print the matrix.
pub fn plan_command(root: &Path, options: PlanOptions) -> Result<()> {
    let config = Config::load(root)?;
    let data = DataDir::new(root);
    let report = compared(&data, &config, options.run)?;
    let plan = plan(&data, &report, &Repo::new(&options.wasmtime_repo))?;
    std::fs::create_dir_all(&options.out)?;
    std::fs::write(options.out.join(PLAN), serde_json::to_string_pretty(&plan)?)?;
    if plan.measures() {
        log::info!(
            "bisecting {} regression(s) over {} candidate(s), measuring {} pair(s) each",
            plan.regressed.len(),
            plan.candidates.len(),
            plan.pairs.len()
        );
    } else {
        log::info!(
            "the range is the single commit {}, so there is nothing to measure",
            short(&plan.candidates[0].commit)
        );
    }
    println!("{}", plan.matrix());
    Ok(())
}

/// The run's comparison, which must have found regressions.
fn compared(data: &DataDir, config: &Config, run: RunNumber) -> Result<Report> {
    match regressions::compare(data, config, run)? {
        Outcome::Compared(r) if !r.regressions.is_empty() => Ok(r),
        Outcome::Compared(_) => bail!("run {run} has no regressions to bisect"),
        Outcome::Skipped { reason, .. } => bail!("run {run} was not compared: {reason}"),
    }
}

/// Plan the bisect of `report`'s regressions.
pub fn plan(data: &DataDir, report: &Report, git: &dyn FirstParents) -> Result<Plan> {
    let regressed: BTreeSet<Regressed> = report
        .regressions
        .iter()
        .map(|c| Regressed {
            benchmark: c.benchmark.clone(),
            phase: c.phase,
        })
        .collect();

    // A Geomean is over every benchmark both runs measured, so reproducing it
    // means measuring all of them, though only in its phase.
    let mut pairs: BTreeSet<Pair> = BTreeSet::new();
    let geomean_phases: Vec<Phase> = regressed
        .iter()
        .filter(|r| r.benchmark == GEOMEAN)
        .map(|r| r.phase)
        .collect();
    if !geomean_phases.is_empty() {
        let measured = |run| -> Result<BTreeSet<String>> {
            Ok(data
                .measurements(run)?
                .iter()
                .map(|m| m.wasm.to_string())
                .collect())
        };
        let (old, new) = (measured(report.baseline)?, measured(report.run)?);
        for phase in geomean_phases {
            for wasm in old.intersection(&new) {
                pairs.insert(Pair {
                    phase,
                    wasm: wasm.clone(),
                });
            }
        }
    }
    for r in regressed.iter().filter(|r| r.benchmark != GEOMEAN) {
        pairs.insert(Pair {
            phase: r.phase,
            wasm: r.benchmark.clone(),
        });
    }

    let candidates = git.first_parents(&report.baseline_commit, &report.commit)?;
    if candidates.is_empty() {
        bail!(
            "no first-parent commits in {}..{}",
            short(&report.baseline_commit),
            short(&report.commit)
        );
    }
    if candidates.len() > MAX_CANDIDATES {
        bail!(
            "{} candidates is more than the {MAX_CANDIDATES} one bisect can measure",
            candidates.len()
        );
    }
    Ok(Plan {
        run: report.run,
        regressed: regressed.into_iter().collect(),
        pairs: pairs.iter().map(Pair::to_string).collect(),
        candidates,
    })
}

/// One candidate's own change in one regressed pair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Measured {
    /// `candidate / parent`, in virtual cycles.
    Ratio(f64),
    /// A side is missing the benchmark, or the whole candidate's results.
    Unmeasured,
}

/// What the bisect concluded about one regressed pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Attribution {
    /// The regressed pair.
    pub regressed: Regressed,
    /// The ratio the main run detected, `new / old`.
    pub detected: f64,
    /// Each candidate's change, in plan order.
    pub measured: Vec<Measured>,
    /// Indices into the plan's candidates of those blamed.
    pub blamed: Vec<usize>,
    /// Did the bisect reproduce the regression?
    pub confirmed: bool,
    /// Do the candidates' changes add up to the detected one?
    pub complete: bool,
}

impl Attribution {
    /// Attribute `detected` among candidates with the given changes.
    ///
    /// Works in log space, where the candidates' changes add up to the range's.
    pub fn new(
        regressed: Regressed,
        detected: f64,
        measured: Vec<Measured>,
        config: &crate::config::Bisect,
    ) -> Self {
        let d = detected.ln();
        let logs: Vec<Option<f64>> = measured
            .iter()
            .map(|m| match *m {
                Measured::Ratio(r) => Some(r.ln()),
                Measured::Unmeasured => None,
            })
            .collect();
        let blamed: Vec<usize> = logs
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_some_and(|c| c >= config.blame_share * d))
            .map(|(i, _)| i)
            .collect();
        let sum: f64 = logs.iter().flatten().sum();
        let tolerance = config.completeness_tolerance * d;
        let all_measured = logs.iter().all(Option::is_some);
        let complete = all_measured && (sum - d).abs() <= tolerance;
        // Several small changes can add up to the detected one with no single
        // commit large enough to blame: creep is still a regression.
        let confirmed = !blamed.is_empty() || sum >= d - tolerance;
        Self {
            regressed,
            detected,
            measured,
            blamed,
            confirmed,
            complete,
        }
    }

    /// The share of the detected change, in log terms, that `m` accounts for.
    fn share(&self, m: Measured) -> Option<f64> {
        match m {
            Measured::Ratio(r) => Some(r.ln() / self.detected.ln()),
            Measured::Unmeasured => None,
        }
    }
}

/// What the bisect found.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// The range is one commit, blamed without measuring.
    SingleCommit,
    /// At least one regressed pair reproduced.
    Confirmed,
    /// Every candidate was measured and none reproduced the regression.
    Unconfirmed,
    /// The bisect could not check: no plan, or nothing measured.
    Failed(String),
}

/// The report's findings over every regressed pair.
#[derive(Clone, Debug)]
pub struct Findings {
    /// The plan, if there was one.
    pub plan: Option<Plan>,
    /// One per regressed pair, in plan order.
    pub attributions: Vec<Attribution>,
    /// The overall verdict.
    pub verdict: Verdict,
}

impl Findings {
    /// The plan's candidates that any pair blamed, newest first.
    fn blamed(&self) -> Vec<&Candidate> {
        let Some(plan) = &self.plan else {
            return vec![];
        };
        let blamed: BTreeSet<usize> = self
            .attributions
            .iter()
            .filter(|a| a.confirmed)
            .flat_map(|a| a.blamed.iter().copied())
            .collect();
        blamed.into_iter().map(|i| &plan.candidates[i]).collect()
    }
}

/// Run `awfy bisect-report`.
pub fn report_command(root: &Path, options: ReportOptions) -> Result<()> {
    let config = Config::load(root)?;
    let data = DataDir::new(root);
    let report = compared(&data, &config, options.run)?;
    let plan = match &options.plan {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => Some(
                serde_json::from_str(&text)
                    .with_context(|| format!("parsing {}", path.display()))?,
            ),
            Err(e) => {
                log::warn!("no plan at {}: {e}", path.display());
                None
            }
        },
        None => None,
    };
    let findings = findings(&config, &report, plan, &options.results)?;
    let log = match &options.wasmtime_repo {
        Some(repo) => Some(Repo::new(repo).log(&report.baseline_commit, &report.commit)?),
        None => None,
    };

    let summary = summary(&report, &findings);
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

    if let (Some(dir), Some(bisection)) = (
        &options.issue_dir,
        bisection(&findings, options.workflow_run.as_deref()),
    ) {
        let metadata = data.metadata(report.run)?;
        crate::issue::write(
            dir,
            &report,
            &metadata,
            &config,
            log.as_deref(),
            Some(&bisection),
        )?;
        log::info!("wrote issue to {}", dir.display());
    }
    Ok(())
}

/// Attribute every regression in `report` from the measurements in `results`.
pub fn findings(
    config: &Config,
    report: &Report,
    plan: Option<Plan>,
    results: &Path,
) -> Result<Findings> {
    let Some(plan) = plan else {
        return Ok(Findings {
            plan: None,
            attributions: vec![],
            verdict: Verdict::Failed("the bisect produced no plan".into()),
        });
    };
    anyhow::ensure!(
        plan.run == report.run,
        "the plan is for run {}, not {}",
        plan.run,
        report.run
    );
    if !plan.measures() {
        return Ok(Findings {
            plan: Some(plan),
            attributions: vec![],
            verdict: Verdict::SingleCommit,
        });
    }

    let mut ratios: Vec<Option<Ratios>> = Vec::new();
    for c in &plan.candidates {
        let dir = results.join(format!("bisect-{}", c.commit));
        ratios.push(match Ratios::read(&dir) {
            Ok(r) => Some(r),
            Err(e) => {
                log::warn!("candidate {} is unmeasured: {e:#}", short(&c.commit));
                None
            }
        });
    }
    if ratios.iter().all(Option::is_none) {
        return Ok(Findings {
            plan: Some(plan),
            attributions: vec![],
            verdict: Verdict::Failed("no candidate was measured".into()),
        });
    }

    let mut attributions = Vec::new();
    for r in &plan.regressed {
        let Some(detected) = report
            .regressions
            .iter()
            .find(|c| c.benchmark == r.benchmark && c.phase == r.phase)
        else {
            bail!(
                "the plan bisects {} {}, which run {} did not regress",
                r.benchmark,
                r.phase,
                report.run
            );
        };
        let measured: Vec<Measured> = ratios
            .iter()
            .map(|c| match c.as_ref().and_then(|c| c.get(r)) {
                Some(ratio) => Measured::Ratio(ratio),
                None => Measured::Unmeasured,
            })
            .collect();
        attributions.push(Attribution::new(
            r.clone(),
            1.0 + detected.relative_change,
            measured,
            &config.bisect,
        ));
    }
    let verdict = if attributions.iter().any(|a| a.confirmed) {
        Verdict::Confirmed
    } else if attributions
        .iter()
        .all(|a| a.measured.iter().all(|m| matches!(m, Measured::Ratio(_))))
    {
        Verdict::Unconfirmed
    } else {
        // Unmeasured candidates might be the ones responsible, so this is no
        // evidence the regression was noise.
        Verdict::Failed("some candidates were unmeasured and the rest did not reproduce it".into())
    };
    Ok(Findings {
        plan: Some(plan),
        attributions,
        verdict,
    })
}

/// One candidate's `candidate / parent` virtual cycles for every (benchmark,
/// phase) measured, Geomeans included.
struct Ratios(BTreeMap<Regressed, f64>);

impl Ratios {
    /// Read `dir/parent/` and `dir/candidate/`, each an `awfy run` scratch
    /// directory.
    fn read(dir: &Path) -> Result<Self> {
        let side = |name: &str| -> Result<Vec<Measurement<'static>>> {
            let path = dir.join(name);
            anyhow::ensure!(path.is_dir(), "{} is missing", path.display());
            let ms = crate::record::merge(&[path])?;
            Ok(ms
                .into_iter()
                .filter(|m| m.event == VIRTUAL_CYCLES)
                .collect())
        };
        Ok(Self::of(&side("parent")?, &side("candidate")?))
    }

    fn of(parent: &[Measurement<'static>], candidate: &[Measurement<'static>]) -> Self {
        let mut ratios = BTreeMap::new();
        let (p, c) = (medians(parent), medians(candidate));
        for (key, &old) in &p {
            if let Some(&new) = c.get(key)
                && old > 0.0
            {
                ratios.insert(key.clone(), new / old);
            }
        }
        // Geomeans the same way the main run computes them, over the
        // benchmarks both sides measured in that phase.
        let shared = |ms: &[Measurement<'static>]| -> Vec<Measurement<'static>> {
            ms.iter()
                .filter(|m| {
                    let key = Regressed {
                        benchmark: m.wasm.to_string(),
                        phase: m.phase,
                    };
                    p.contains_key(&key) && c.contains_key(&key)
                })
                .cloned()
                .collect()
        };
        let geomeans = |ms: &[Measurement<'static>]| -> BTreeMap<Phase, f64> {
            geomean::summarize(ms)
                .into_iter()
                .map(|g| (g.phase, g.geomean))
                .collect()
        };
        let (gp, gc) = (geomeans(&shared(parent)), geomeans(&shared(candidate)));
        for (phase, old) in gp {
            if let Some(new) = gc.get(&phase) {
                ratios.insert(
                    Regressed {
                        benchmark: GEOMEAN.into(),
                        phase,
                    },
                    new / old,
                );
            }
        }
        Self(ratios)
    }

    fn get(&self, r: &Regressed) -> Option<f64> {
        self.0.get(r).copied()
    }
}

/// Each (benchmark, phase)'s median count.
fn medians(ms: &[Measurement<'_>]) -> BTreeMap<Regressed, f64> {
    let mut counts: BTreeMap<Regressed, Vec<u64>> = BTreeMap::new();
    for m in ms {
        counts
            .entry(Regressed {
                benchmark: m.wasm.to_string(),
                phase: m.phase,
            })
            .or_default()
            .push(m.count);
    }
    counts
        .into_iter()
        .map(|(k, mut v)| {
            v.sort_unstable();
            let n = v.len();
            let median = if n % 2 == 1 {
                v[n / 2] as f64
            } else {
                (v[n / 2 - 1] as f64 + v[n / 2] as f64) / 2.0
            };
            (k, median)
        })
        .collect()
}

/// What to add to the issue, or `None` to file nothing.
pub fn bisection(findings: &Findings, workflow_run: Option<&str>) -> Option<Bisection> {
    let mut s = String::new();
    match &findings.verdict {
        Verdict::Unconfirmed => return None,
        Verdict::Failed(why) => {
            writeln!(
                s,
                "The bisect could not attribute this regression ({why}), so it \
                 names the whole range.{}\n",
                workflow_link(workflow_run)
            )
            .unwrap();
            return Some(Bisection {
                title: None,
                section: s,
                reproduce: None,
            });
        }
        Verdict::SingleCommit => {
            let plan = findings.plan.as_ref().expect("a single commit has a plan");
            let [one] = plan.candidates.as_slice() else {
                unreachable!("a single-commit plan has one candidate");
            };
            writeln!(
                s,
                "The range is the single commit {}, so it is blamed without \
                 bisecting.{}\n",
                markdown::commit_link(&one.commit, &one.subject),
                workflow_link(workflow_run)
            )
            .unwrap();
            // The range is already `one`'s parent to `one`, so the default
            // reproduce section fits.
            return Some(Bisection {
                title: Some(commit_title(one)),
                section: s,
                reproduce: None,
            });
        }
        Verdict::Confirmed => {}
    }
    let plan = findings
        .plan
        .as_ref()
        .expect("a confirmed bisect has a plan");
    let blamed = findings.blamed();
    match blamed.as_slice() {
        [] => writeln!(
            s,
            "Bisecting the range found no single commit responsible: the \
             regression is several small changes adding up.{}\n",
            workflow_link(workflow_run)
        ),
        [one] => writeln!(
            s,
            "Bisecting the range blames {}.{}\n",
            markdown::commit_link(&one.commit, &one.subject),
            workflow_link(workflow_run)
        ),
        many => {
            writeln!(s, "Bisecting the range blames {} commits:\n", many.len()).unwrap();
            for c in many {
                writeln!(s, "* {}", markdown::commit_link(&c.commit, &c.subject)).unwrap();
            }
            match workflow_run {
                Some(_) => writeln!(s, "\n{}\n", workflow_link(workflow_run).trim_start()),
                None => writeln!(s),
            }
        }
    }
    .unwrap();
    attribution_table(&mut s, findings, plan);

    let title = match blamed.as_slice() {
        [] => None,
        [one] => Some(commit_title(one)),
        many => Some(format!("{} commits", many.len())),
    };
    let reproduce = match blamed.as_slice() {
        [one] => {
            let index = plan.candidates.iter().position(|c| c == *one).unwrap();
            let wasms: Vec<String> = findings
                .attributions
                .iter()
                .filter(|a| a.blamed.contains(&index))
                .map(|a| a.regressed.benchmark.clone())
                .collect();
            Some(((*one).clone(), wasms))
        }
        _ => None,
    };
    Some(Bisection {
        title,
        section: s,
        reproduce,
    })
}

/// Each regressed pair, then each candidate's share of it.
fn attribution_table(s: &mut String, findings: &Findings, plan: &Plan) {
    for a in &findings.attributions {
        writeln!(
            s,
            "#### `{}` {}: {} detected\n",
            a.regressed.benchmark,
            a.regressed.phase,
            signed_percent(a.detected - 1.0)
        )
        .unwrap();
        let note = if !a.confirmed {
            "No commit reproduced this one; it may have been noise."
        } else if a.complete {
            "The commits' own changes account for the whole regression."
        } else {
            "The commits' own changes don't add up to the whole regression; \
             the rest is unexplained."
        };
        writeln!(s, "{note}\n").unwrap();
        writeln!(s, "| commit | change | share | |").unwrap();
        writeln!(s, "|--------|--------|-------|-|").unwrap();
        for (i, (c, &m)) in plan.candidates.iter().zip(&a.measured).enumerate() {
            let (change, share) = match (m, a.share(m)) {
                (Measured::Ratio(r), Some(share)) => {
                    (signed_percent(r - 1.0), format!("{:.0}%", share * 100.0))
                }
                _ => ("unmeasured".into(), "".into()),
            };
            let verdict = if a.confirmed && a.blamed.contains(&i) {
                "**blamed**"
            } else {
                ""
            };
            writeln!(
                s,
                "| {} | {change} | {share} | {verdict} |",
                markdown::commit_link(&c.commit, &c.subject)
            )
            .unwrap();
        }
        writeln!(s).unwrap();
    }
}

/// The issue title's name for one blamed commit, e.g. `0123456789ab (#15208)`.
fn commit_title(c: &Candidate) -> String {
    match pr_number(&c.subject) {
        Some(pr) => format!("{} ({pr})", short(&c.commit)),
        None => short(&c.commit).to_string(),
    }
}

/// ` See the [bisect](url).`, or nothing.
fn workflow_link(url: Option<&str>) -> String {
    match url {
        Some(url) => format!(" See the [bisect]({url})."),
        None => String::new(),
    }
}

/// The `#NNNNN` a merged PR's subject ends with, as in `Fix it (#15208)`.
fn pr_number(subject: &str) -> Option<&str> {
    let start = subject.rfind("(#")?;
    let pr = subject[start + 1..].strip_suffix(')')?;
    pr[1..].bytes().all(|b| b.is_ascii_digit()).then_some(pr)
}

/// The job summary.
fn summary(report: &Report, findings: &Findings) -> String {
    let mut s = format!("## Bisect: run {} vs. {}\n\n", report.run, report.baseline);
    let outcome = match &findings.verdict {
        Verdict::SingleCommit => {
            "The range is a single commit: filing the issue without bisecting.".to_string()
        }
        Verdict::Confirmed => "Confirmed: filing the issue.".to_string(),
        Verdict::Unconfirmed => "Unconfirmed detection: no commit reproduced the regression, so \
             nothing is filed."
            .to_string(),
        Verdict::Failed(why) => format!("The bisect failed ({why}): filing the range issue."),
    };
    writeln!(s, "{outcome}\n").unwrap();
    if let Some(plan) = &findings.plan {
        attribution_table(&mut s, findings, plan);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::assert_snapshot;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn config() -> Config {
        Config::load(&fixtures()).unwrap()
    }

    /// The fixture comparison of 00003 with 00002, which regresses bz2 and
    /// the Geomean in execution.
    fn fixture_report() -> Report {
        compared(
            &DataDir::new(&fixtures()),
            &config(),
            RunNumber::new(3).unwrap(),
        )
        .unwrap()
    }

    fn candidate(i: usize) -> Candidate {
        let sha = |i: usize| format!("{:x}", 0xc0 + i).repeat(20);
        Candidate {
            commit: sha(i),
            parent: sha(i + 1),
            subject: format!("Change number {i} (#{})", 15200 + i),
        }
    }

    /// Candidates 0 (newest) to `n - 1`, chained parent to child.
    struct FakeGit(usize);

    impl FirstParents for FakeGit {
        fn first_parents(&self, old: &str, new: &str) -> Result<Vec<Candidate>> {
            assert_eq!(old, "2".repeat(40));
            assert_eq!(new, "3".repeat(40));
            Ok((0..self.0).map(candidate).collect())
        }
    }

    fn regressed(benchmark: &str, phase: Phase) -> Regressed {
        Regressed {
            benchmark: benchmark.into(),
            phase,
        }
    }

    fn attribute(detected: f64, ratios: &[Option<f64>]) -> Attribution {
        let measured = ratios
            .iter()
            .map(|r| match r {
                Some(r) => Measured::Ratio(*r),
                None => Measured::Unmeasured,
            })
            .collect();
        Attribution::new(
            regressed("a.wasm", Phase::Execution),
            detected,
            measured,
            &config().bisect,
        )
    }

    #[test]
    fn single_culprit() {
        let a = attribute(1.02, &[Some(1.0), Some(1.02), Some(0.99999)]);
        assert_eq!(a.blamed, [1]);
        assert!(a.confirmed && a.complete);
    }

    #[test]
    fn two_partial_culprits() {
        let a = attribute(1.02, &[Some(1.01), Some(1.0), Some(1.0099)]);
        assert_eq!(a.blamed, [0, 2]);
        assert!(a.confirmed && a.complete);
    }

    /// Five changes of 0.4% add up to a 2% regression, none of them big
    /// enough alone to blame.
    #[test]
    fn creep() {
        let a = attribute(1.02, &[Some(1.004); 5]);
        assert!(a.blamed.is_empty());
        assert!(a.confirmed && a.complete);
    }

    /// Nothing moved: the detection was noise.
    #[test]
    fn noise() {
        let a = attribute(1.02, &[Some(1.0001), Some(0.9999), Some(1.0)]);
        assert!(a.blamed.is_empty());
        assert!(!a.confirmed);
    }

    #[test]
    fn incomplete() {
        let a = attribute(1.04, &[Some(1.0), Some(1.015)]);
        assert_eq!(a.blamed, [1]);
        assert!(a.confirmed && !a.complete);
    }

    /// One commit regressed and another sped things back up: both net out to
    /// the detected change, and only the regression is blamed.
    #[test]
    fn partial_undo() {
        let a = attribute(1.01, &[Some(0.99), Some(1.0202)]);
        assert_eq!(a.blamed, [1]);
        assert!(a.confirmed && a.complete);
    }

    #[test]
    fn unmeasured_is_never_complete() {
        let a = attribute(1.02, &[None, Some(1.02)]);
        assert_eq!(a.blamed, [1]);
        assert!(a.confirmed && !a.complete);
    }

    #[test]
    fn pr_numbers() {
        assert_eq!(pr_number("Fix it (#15208)"), Some("#15208"));
        assert_eq!(pr_number("Fix it"), None);
        assert_eq!(pr_number("Fix (#x)"), None);
        assert_eq!(pr_number("(#1) not at the end"), None);
    }

    #[test]
    fn plans_the_fixture_regression() {
        let report = fixture_report();
        let plan = plan(&DataDir::new(&fixtures()), &report, &FakeGit(3)).unwrap();
        assert_eq!(
            plan.regressed,
            [
                regressed(GEOMEAN, Phase::Execution),
                regressed("bz2/benchmark.wasm", Phase::Execution)
            ]
        );
        // The Geomean needs every benchmark, but only in execution.
        assert_eq!(
            plan.pairs,
            [
                "execution:bz2/benchmark.wasm",
                "execution:pulldown-cmark/benchmark.wasm"
            ]
        );
        assert_eq!(plan.candidates.len(), 3);
        let matrix = plan.matrix();
        assert_eq!(matrix["include"].as_array().unwrap().len(), 3);
        assert_eq!(
            matrix["include"][1],
            serde_json::json!({
                "commit": candidate(1).commit,
                "parent": candidate(1).parent,
                "pairs": "execution:bz2/benchmark.wasm execution:pulldown-cmark/benchmark.wasm",
            })
        );
        // Round trips through the file the report reads.
        let text = serde_json::to_string(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
    }

    #[test]
    fn too_many_candidates() {
        let report = fixture_report();
        let err = plan(
            &DataDir::new(&fixtures()),
            &report,
            &FakeGit(MAX_CANDIDATES + 1),
        )
        .unwrap_err();
        assert!(err.to_string().contains("more than the 256"), "{err}");
    }

    #[test]
    fn nothing_to_bisect() {
        let err = compared(
            &DataDir::new(&fixtures()),
            &config(),
            RunNumber::new(2).unwrap(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no regressions"), "{err}");
    }

    fn measurement(wasm: &str, phase: Phase, count: u64) -> Measurement<'static> {
        Measurement {
            arch: "x86_64".into(),
            engine: sightglass_data::Engine::from("wasmtime".to_string()),
            wasm: wasm.to_string().into(),
            phase,
            event: VIRTUAL_CYCLES.into(),
            count,
        }
    }

    #[test]
    fn ratios_include_geomeans() {
        let side = |a: u64, b: u64| {
            vec![
                measurement("a.wasm", Phase::Execution, a),
                measurement("a.wasm", Phase::Execution, a + 2),
                measurement("b.wasm", Phase::Execution, b),
                measurement("b.wasm", Phase::Execution, b + 2),
            ]
        };
        let ratios = Ratios::of(&side(999, 1999), &side(1099, 1999));
        let get = |b: &str| ratios.get(&regressed(b, Phase::Execution)).unwrap();
        assert!((get("a.wasm") - 1.1).abs() < 1e-12);
        assert_eq!(get("b.wasm"), 1.0);
        // sqrt(1.1 * 1.0).
        assert!((get(GEOMEAN) - 1.1f64.sqrt()).abs() < 1e-12);
        assert_eq!(ratios.get(&regressed("a.wasm", Phase::Compilation)), None);
    }

    /// Findings over three candidates, given each one's ratio for both of the
    /// fixture's regressed pairs.
    fn fixture_findings(ratios: [Option<f64>; 3]) -> (Report, Findings) {
        let report = fixture_report();
        let plan = plan(&DataDir::new(&fixtures()), &report, &FakeGit(3)).unwrap();
        let attributions = plan
            .regressed
            .iter()
            .map(|r| {
                let detected = report
                    .regressions
                    .iter()
                    .find(|c| c.benchmark == r.benchmark && c.phase == r.phase)
                    .unwrap();
                // The Geomean is over two benchmarks, so moves half as much
                // in log terms as bz2 alone.
                let geomean = detected.benchmark == GEOMEAN;
                let measured = ratios
                    .iter()
                    .map(|r| match r {
                        Some(x) if geomean => Measured::Ratio(x.sqrt()),
                        Some(x) => Measured::Ratio(*x),
                        None => Measured::Unmeasured,
                    })
                    .collect();
                Attribution::new(
                    r.clone(),
                    1.0 + detected.relative_change,
                    measured,
                    &config().bisect,
                )
            })
            .collect::<Vec<_>>();
        let verdict = if attributions.iter().any(|a| a.confirmed) {
            Verdict::Confirmed
        } else {
            Verdict::Unconfirmed
        };
        (
            report,
            Findings {
                plan: Some(plan),
                attributions,
                verdict,
            },
        )
    }

    fn render(report: &Report, findings: &Findings) -> String {
        let data = DataDir::new(&fixtures());
        let metadata = data.metadata(report.run).unwrap();
        let dir = tempfile::tempdir().unwrap();
        // As `report_command` does: no bisection, no issue.
        let Some(bisection) = bisection(findings, Some("https://example.com/bisect/1")) else {
            return String::new();
        };
        let wrote = crate::issue::write(
            dir.path(),
            report,
            &metadata,
            &config(),
            None,
            Some(&bisection),
        )
        .unwrap();
        assert!(wrote);
        let title = std::fs::read_to_string(dir.path().join("title.txt")).unwrap();
        let body = std::fs::read_to_string(dir.path().join("body.md")).unwrap();
        format!("{title}\n{body}")
    }

    #[test]
    fn one_culprit_issue() {
        let (report, findings) = fixture_findings([Some(1.0), Some(1.015), Some(1.0)]);
        let issue = render(&report, &findings);
        assert!(
            issue.starts_with(&format!(
                "Performance regression in {} (#15201) (are-we-fast-yet run 00003)\n",
                short(&candidate(1).commit)
            )),
            "{issue}"
        );
        assert_snapshot("bisect/one.md", &issue);
        assert_snapshot("bisect/one-summary.md", &summary(&report, &findings));
    }

    #[test]
    fn two_culprit_issue() {
        let (report, findings) = fixture_findings([Some(1.0075), Some(1.0), Some(1.0075)]);
        let issue = render(&report, &findings);
        assert!(
            issue.starts_with("Performance regression in 2 commits (are-we-fast-yet run 00003)\n"),
            "{issue}"
        );
        assert_snapshot("bisect/two.md", &issue);
    }

    #[test]
    fn noise_files_nothing() {
        let (report, findings) = fixture_findings([Some(1.0), Some(1.0001), Some(0.9999)]);
        assert_eq!(findings.verdict, Verdict::Unconfirmed);
        assert_eq!(render(&report, &findings), "");
        assert_snapshot("bisect/noise-summary.md", &summary(&report, &findings));
    }

    #[test]
    fn single_commit_files_without_measuring() {
        let report = fixture_report();
        let plan = plan(&DataDir::new(&fixtures()), &report, &FakeGit(1)).unwrap();
        assert!(!plan.measures());
        assert_eq!(plan.matrix(), serde_json::json!({ "include": [] }));

        // No results at all.
        let tmp = tempfile::tempdir().unwrap();
        let findings = findings(&config(), &report, Some(plan), tmp.path()).unwrap();
        assert_eq!(findings.verdict, Verdict::SingleCommit);
        let issue = render(&report, &findings);
        assert!(
            issue.starts_with(&format!(
                "Performance regression in {} (#15200) (are-we-fast-yet run 00003)\n",
                short(&candidate(0).commit)
            )),
            "{issue}"
        );
        assert_snapshot("bisect/single.md", &issue);
        assert_snapshot("bisect/single-summary.md", &summary(&report, &findings));
    }

    #[test]
    fn failed_bisect_files_the_range() {
        let report = fixture_report();
        let tmp = tempfile::tempdir().unwrap();
        let findings = findings(&config(), &report, None, tmp.path()).unwrap();
        assert!(matches!(findings.verdict, Verdict::Failed(_)));
        let issue = render(&report, &findings);
        assert!(
            issue.starts_with(
                "Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)\n"
            ),
            "{issue}"
        );
        assert!(issue.contains("the bisect produced no plan"), "{issue}");

        // A plan, but no candidate's results.
        let plan = plan(&DataDir::new(&fixtures()), &report, &FakeGit(2)).unwrap();
        let findings = super::findings(&config(), &report, Some(plan), tmp.path()).unwrap();
        assert_eq!(
            findings.verdict,
            Verdict::Failed("no candidate was measured".into())
        );
    }
}
