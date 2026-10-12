//! Render a regression [`Report`] as a GitHub issue. See
//! `docs/regressions.md` ("Issue").

use crate::config::Config;
use crate::metadata::Metadata;
use crate::regressions::{short, Change, Report, VIRTUAL_CYCLES};
use crate::runs::RunNumber;
use crate::slug::slug;
use anyhow::{Context, Result};
use sightglass_analysis::geomean::GEOMEAN;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The Wasmtime repository URL on github.
pub const WASMTIME_REPO: &str = "https://github.com/bytecodealliance/wasmtime";

/// Most commits listed in the body.
pub const MAX_COMMITS: usize = 50;

/// An issue to be created.
pub struct Issue {
    /// The issue's title.
    pub title: String,
    /// The issue's body.
    pub body: String,
}

/// The title suffix that identifies a run's issue. The workflow searches for
/// it before filing, so re-runs never file duplicates.
pub fn issue_key(run: RunNumber) -> String {
    format!("(are-we-fast-yet run {run})")
}

/// A trait to run `git log`, so we can fake it in tests.
pub trait GitLog {
    /// Run `git log --oneline old..new`.
    fn log(&self, old: &str, new: &str) -> Result<Vec<String>>;
}

/// A local Wasmtime clone.
///
/// A treeless checkout is good enough for our purposes.
pub struct Repo(pub PathBuf);

impl GitLog for Repo {
    fn log(&self, old: &str, new: &str) -> Result<Vec<String>> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args([
                "log",
                "--oneline",
                "--no-decorate",
                &format!("{old}..{new}"),
            ])
            .output()
            .context("running `git log`")?;
        anyhow::ensure!(
            output.status.success(),
            "`git log` in {} failed: {}",
            self.0.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?
            .lines()
            .map(String::from)
            .collect())
    }
}

/// Render the issue, or `None` if there are no regressions.
pub fn render(
    report: &Report,
    metadata: &Metadata,
    config: &Config,
    git: Option<&dyn GitLog>,
) -> Result<Option<Issue>> {
    if report.regressions.is_empty() {
        return Ok(None);
    }
    let site = &config.site.base_url;
    let (old, new) = (&report.baseline_commit, &report.commit);
    let title = format!(
        "Performance regression in {}..{} {}",
        short(old),
        short(new),
        issue_key(report.run)
    );

    let mut b = String::new();
    let run_link = |run: RunNumber| {
        format!(
            "[run {run}]({site}run.html?run={run}) ([metadata](https://github.com/{}/blob/main/benchmark-data/{run}/metadata.toml))",
            config.site.repo
        )
    };
    writeln!(
        b,
        "[are-we-fast-yet](https://github.com/{}) found {} in virtual cycles \
         (callgrind) between Wasmtime `{}` and `{}`.\n",
        config.site.repo,
        plural(report.regressions.len(), "regression", "regressions"),
        short(old),
        short(new),
    )?;
    writeln!(b, "* Baseline: {}", run_link(report.baseline))?;
    writeln!(b, "* This run: {}", run_link(report.run))?;
    writeln!(
        b,
        "* Threshold: significant at the {} level and at least {}% more\n",
        report.significance_level,
        report.min_relative_change * 100.0
    )?;

    writeln!(b, "| benchmark | phase | effect size |")?;
    writeln!(b, "|-----------|-------|-------------|")?;
    for c in &report.regressions {
        writeln!(
            b,
            "| [`{}`]({site}benchmark.html?b={}&metric={VIRTUAL_CYCLES}) | {} | {} |",
            c.benchmark,
            slug(&c.benchmark),
            c.phase,
            effect(c)
        )?;
    }
    writeln!(b)?;

    if !report.other_counters.is_empty() {
        writeln!(b, "<details>")?;
        writeln!(
            b,
            "<summary>Other callgrind counters that changed for these benchmarks</summary>\n"
        )?;
        writeln!(b, "| benchmark | phase | effect size |")?;
        writeln!(b, "|-----------|-------|-------------|")?;
        for c in &report.other_counters {
            writeln!(b, "| `{}` | {} | {} |", c.benchmark, c.phase, effect(c))?;
        }
        writeln!(b, "\n</details>\n")?;
    }

    writeln!(b, "### Commits\n")?;
    writeln!(b, "{WASMTIME_REPO}/compare/{old}...{new}\n")?;
    if let Some(git) = git {
        let commits = git.log(old, new)?;
        writeln!(b, "```")?;
        for line in commits.iter().take(MAX_COMMITS) {
            writeln!(b, "{line}")?;
        }
        if commits.len() > MAX_COMMITS {
            writeln!(b, "... and {} more", commits.len() - MAX_COMMITS)?;
        }
        writeln!(b, "```\n")?;
    }

    reproduce(&mut b, report, metadata)?;
    Ok(Some(Issue { title, body: b }))
}

/// E.g. `1.23x +/- 0.004 more virtual cycles`.
fn effect(c: &Change) -> String {
    format!(
        "{:.3}x +/- {:.3} {} {}",
        c.ratio,
        c.ratio_ci,
        if c.is_increase() { "more" } else { "fewer" },
        c.event.replace('-', " ")
    )
}

fn reproduce(b: &mut String, report: &Report, metadata: &Metadata) -> Result<()> {
    let mut wasms: Vec<&str> = report
        .regressions
        .iter()
        .map(|c| c.benchmark.as_str())
        .filter(|&w| w != GEOMEAN)
        .collect();
    wasms.sort();
    wasms.dedup();
    if wasms.is_empty() {
        // Only the Geomean regressed: rerun the whole suite.
        wasms.push(&metadata.sightglass.suite);
    }
    let wasms: Vec<String> = wasms
        .iter()
        .map(|w| {
            if w.ends_with(".suite") {
                w.to_string()
            } else {
                format!("benchmarks/{w}")
            }
        })
        .collect();

    writeln!(b, "### Reproduce locally\n")?;
    writeln!(b, "On x86-64 Linux with Valgrind installed:\n")?;
    writeln!(b, "```sh")?;
    writeln!(
        b,
        "git clone https://github.com/bytecodealliance/sightglass"
    )?;
    writeln!(b, "cd sightglass")?;
    writeln!(b, "git checkout {}", metadata.sightglass.commit)?;
    writeln!(b, "rustc engines/wasmtime/build.rs -o build-engine")?;
    writeln!(b, "mkdir old new")?;
    writeln!(b, "REVISION={} ./build-engine old", report.baseline_commit)?;
    writeln!(b, "REVISION={} ./build-engine new", report.commit)?;
    writeln!(b, "cargo run --release -- benchmark --measure callgrind \\")?;
    writeln!(
        b,
        "  --engine old/libengine.so --engine new/libengine.so \\"
    )?;
    writeln!(
        b,
        "  --engine-flags-common=\"{}\" \\",
        metadata.config.engine_flags
    )?;
    writeln!(b, "  -- {}", wasms.join(" "))?;
    writeln!(b, "```")?;
    Ok(())
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Write `title.txt` and `body.md` to `dir` if there are regressions.
pub fn write(
    dir: &Path,
    report: &Report,
    metadata: &Metadata,
    config: &Config,
    wasmtime_repo: Option<&Path>,
) -> Result<bool> {
    let repo = wasmtime_repo.map(|p| Repo(p.to_path_buf()));
    let git = repo.as_ref().map(|r| r as &dyn GitLog);
    let Some(issue) = render(report, metadata, config, git)? else {
        return Ok(false);
    };
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join("title.txt"), format!("{}\n", issue.title))?;
    std::fs::write(dir.join("body.md"), &issue.body)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regressions::{compare, Outcome};
    use crate::runs::DataDir;
    use crate::snapshot::assert_snapshot;
    use sightglass_data::Phase;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn config() -> Config {
        Config::load(&fixtures()).unwrap()
    }

    /// The fixture comparison of 00003 against 00002, and 00003's metadata.
    fn fixture_report() -> (Report, Metadata) {
        let data = DataDir::new(&fixtures());
        let Outcome::Compared(report) =
            compare(&data, &config(), RunNumber::new(3).unwrap()).unwrap()
        else {
            panic!("skipped");
        };
        (report, data.metadata(RunNumber::new(3).unwrap()).unwrap())
    }

    struct FakeGit(usize);

    impl GitLog for FakeGit {
        fn log(&self, _old: &str, _new: &str) -> Result<Vec<String>> {
            Ok((0..self.0)
                .rev()
                .map(|i| format!("{:07x} Commit number {i}", 0xabc0000 + i))
                .collect())
        }
    }

    fn change(benchmark: &str, phase: Phase, event: &str, old: f64, new: f64) -> Change {
        let ratio = if new >= old { new / old } else { old / new };
        Change {
            benchmark: benchmark.into(),
            phase,
            event: event.into(),
            old_mean: old,
            new_mean: new,
            relative_change: (new - old) / old,
            ratio,
            ratio_ci: 0.004,
        }
    }

    fn render_snapshot(name: &str, report: &Report, metadata: &Metadata, git: Option<&dyn GitLog>) {
        let issue = render(report, metadata, &config(), git).unwrap().unwrap();
        assert_snapshot(
            &format!("issue/{name}.md"),
            &format!("{}\n\n{}", issue.title, issue.body),
        );
    }

    #[test]
    fn title() {
        let (report, metadata) = fixture_report();
        let issue = render(&report, &metadata, &config(), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            issue.title,
            "Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)"
        );
        assert!(issue
            .title
            .ends_with(&issue_key(RunNumber::new(3).unwrap())));
    }

    #[test]
    fn one_regression() {
        let (report, metadata) = fixture_report();
        render_snapshot("one", &report, &metadata, Some(&FakeGit(3)));
    }

    #[test]
    fn no_wasmtime_repo() {
        let (report, metadata) = fixture_report();
        render_snapshot("no-repo", &report, &metadata, None);
    }

    #[test]
    fn many_regressions_and_truncated_log() {
        let (mut report, metadata) = fixture_report();
        report.regressions = vec![
            change(GEOMEAN, Phase::Execution, VIRTUAL_CYCLES, 1e6, 1.05e6),
            change(
                "spidermonkey/spidermonkey-json.wasm",
                Phase::Compilation,
                VIRTUAL_CYCLES,
                5e9,
                5.5e9,
            ),
            change(
                "bz2/benchmark.wasm",
                Phase::Execution,
                VIRTUAL_CYCLES,
                2e6,
                2.03e6,
            ),
            change(
                "bz2/benchmark.wasm",
                Phase::Compilation,
                VIRTUAL_CYCLES,
                1e6,
                1.02e6,
            ),
        ];
        report.other_counters = vec![
            change(
                "bz2/benchmark.wasm",
                Phase::Execution,
                "l1-dcache-read-misses",
                1e4,
                2e4,
            ),
            change(
                "bz2/benchmark.wasm",
                Phase::Execution,
                "conditional-branches",
                1e5,
                0.9e5,
            ),
        ];
        render_snapshot("many", &report, &metadata, Some(&FakeGit(60)));
    }

    #[test]
    fn geomean_only() {
        let (mut report, metadata) = fixture_report();
        report.regressions = vec![change(
            GEOMEAN,
            Phase::Execution,
            VIRTUAL_CYCLES,
            1e6,
            1.012e6,
        )];
        report.other_counters.clear();
        render_snapshot("geomean", &report, &metadata, Some(&FakeGit(1)));
    }

    #[test]
    fn no_regressions() {
        let (mut report, metadata) = fixture_report();
        report.regressions.clear();
        assert!(render(&report, &metadata, &config(), None)
            .unwrap()
            .is_none());
        let dir = tempfile::tempdir().unwrap();
        let issue_dir = dir.path().join("issue");
        assert!(!write(&issue_dir, &report, &metadata, &config(), None).unwrap());
        assert!(!issue_dir.exists());
    }

    #[test]
    fn real_git_log() {
        let tmp = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .arg("-C")
                .arg(tmp.path())
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "commit.gpgsign=false",
                ])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_string()
        };
        git(&["init", "-q"]);
        let mut shas = Vec::new();
        for i in 0..4 {
            git(&[
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("Commit {i}"),
            ]);
            shas.push(git(&["rev-parse", "HEAD"]));
        }
        let log = Repo(tmp.path().to_path_buf())
            .log(&shas[0], &shas[3])
            .unwrap();
        let messages: Vec<&str> = log.iter().map(|l| l.split_once(' ').unwrap().1).collect();
        assert_eq!(messages, ["Commit 3", "Commit 2", "Commit 1"]);
    }
}
