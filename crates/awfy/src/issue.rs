//! Render a regression [`Report`] as a GitHub issue. See
//! `docs/regressions.md` ("Issue").

use crate::config::Config;
use crate::git::Commit;
use crate::markdown::{self, short};
use crate::metadata::Metadata;
use crate::regressions::Report;
use crate::runs::RunNumber;
use anyhow::Result;
use sightglass_analysis::geomean::GEOMEAN;
use std::fmt::Write as _;
use std::path::Path;

/// An issue to be created.
struct Issue {
    /// The issue's title.
    title: String,
    /// The issue's body.
    body: String,
}

/// The title suffix that identifies a run's issue. The workflow searches for
/// it before filing, so re-runs never file duplicates.
pub fn issue_key(run: RunNumber) -> String {
    format!("(are-we-fast-yet run {run})")
}

/// Render the issue, or `None` if there are no regressions. Speedups alone
/// never file one, but are listed beside the regressions they came with.
fn render(
    report: &Report,
    metadata: &Metadata,
    config: &Config,
    log: Option<&[Commit]>,
) -> Result<Option<Issue>> {
    if report.regressions.is_empty() {
        return Ok(None);
    }
    let (old, new) = (&report.baseline_commit, &report.commit);
    let title = format!(
        "Performance regression in {}..{} {}",
        short(old),
        short(new),
        issue_key(report.run)
    );

    let mut b = String::new();
    let speedups = if report.speedups.is_empty() {
        String::new()
    } else {
        format!(
            " and {}",
            plural(report.speedups.len(), "speedup", "speedups")
        )
    };
    writeln!(
        b,
        "[are-we-fast-yet](https://github.com/{}) found {}{speedups} in virtual \
         cycles (callgrind) between Wasmtime `{}` and `{}`.\n",
        config.site.repo,
        plural(report.regressions.len(), "regression", "regressions"),
        short(old),
        short(new),
    )?;
    markdown::runs(&mut b, report, config)?;
    markdown::change_table(&mut b, config, "Regressions", &report.regressions)?;
    if !report.speedups.is_empty() {
        markdown::change_table(&mut b, config, "Speedups", &report.speedups)?;
    }
    markdown::notes(&mut b, config)?;
    markdown::other_counters(&mut b, &report.other_counters)?;
    markdown::commits(&mut b, old, new, log)?;
    reproduce(&mut b, report, metadata)?;
    Ok(Some(Issue { title, body: b }))
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

/// Write `title.txt` and `body.md` to `dir` if there are regressions. `log` is
/// the Wasmtime commits between the two runs, if known.
pub fn write(
    dir: &Path,
    report: &Report,
    metadata: &Metadata,
    config: &Config,
    log: Option<&[Commit]>,
) -> Result<bool> {
    let Some(issue) = render(report, metadata, config, log)? else {
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
    use crate::regressions::{Change, Outcome, VIRTUAL_CYCLES, compare};
    use crate::runs::DataDir;
    use crate::snapshot::assert_snapshot;
    use sightglass_data::Phase;
    use std::path::PathBuf;

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

    /// Subjects that look like Wasmtime's, cycled to make as many as needed.
    const SUBJECTS: &[&str] = &[
        "Cranelift: add an egraph rule for `x * 2`",
        "Revert \"Bump the MSRV to 1.84\"",
        "winch: fix the stack alignment on aarch64",
        "Bump wasm-tools to 1.220.0",
    ];

    /// `n` commits, newest first.
    fn fake_log(n: usize) -> Vec<Commit> {
        (0..n)
            .rev()
            .map(|i| Commit {
                commit: format!("{:x}{}", 0xabc0000 + i, "f".repeat(33)),
                subject: SUBJECTS[i % SUBJECTS.len()].to_string(),
            })
            .collect()
    }

    fn change(benchmark: &str, phase: Phase, event: &str, old: f64, new: f64) -> Change {
        let ratio = if new >= old { new / old } else { old / new };
        let geomean = benchmark == GEOMEAN;
        Change {
            benchmark: benchmark.into(),
            phase,
            event: event.into(),
            old,
            new,
            relative_change: (new - old) / old,
            ratio,
            ratio_low: ratio - 0.004,
            ratio_high: ratio + 0.004,
            // The suite geomean has no samples of its own, so no noise.
            old_noise: (!geomean).then_some(0.012),
            new_noise: (!geomean).then_some(0.015),
            instructions: (!geomean).then_some(crate::regressions::Instructions {
                change: (new - old) / old * 0.8,
                old_noise: 0.009,
                new_noise: 0.011,
            }),
        }
    }

    fn render_snapshot(name: &str, report: &Report, metadata: &Metadata, log: Option<&[Commit]>) {
        let issue = render(report, metadata, &config(), log).unwrap().unwrap();
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
        assert!(
            issue
                .title
                .ends_with(&issue_key(RunNumber::new(3).unwrap()))
        );
    }

    #[test]
    fn one_regression() {
        let (report, metadata) = fixture_report();
        render_snapshot("one", &report, &metadata, Some(&fake_log(3)));
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
        report.speedups = vec![
            change(GEOMEAN, Phase::Compilation, VIRTUAL_CYCLES, 1e9, 0.97e9),
            change(
                "pulldown-cmark/benchmark.wasm",
                Phase::Compilation,
                VIRTUAL_CYCLES,
                2e8,
                1.9e8,
            ),
        ];
        render_snapshot("many", &report, &metadata, Some(&fake_log(60)));
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
        report.speedups.clear();
        report.other_counters.clear();
        render_snapshot("geomean", &report, &metadata, Some(&fake_log(1)));
    }

    /// A subject is arbitrary text, so it must not break out of the link.
    #[test]
    fn commit_subjects_are_escaped() {
        let log = [Commit {
            commit: "a".repeat(40),
            subject: r"Return Box<dyn Error> & fix [brackets](x) and \backslashes".into(),
        }];
        let (report, metadata) = fixture_report();
        let issue = render(&report, &metadata, &config(), Some(&log))
            .unwrap()
            .unwrap();
        assert!(
            issue.body.contains(&format!(
                r"* [`aaaaaaaaaaaa` Return Box&lt;dyn Error&gt; &amp; fix \[brackets\](x) and \\backslashes](https://github.com/bytecodealliance/wasmtime/commit/{})",
                "a".repeat(40)
            )),
            "{}",
            issue.body
        );
    }

    /// Speedups alone never file an issue.
    #[test]
    fn no_regressions() {
        let (mut report, metadata) = fixture_report();
        report.regressions.clear();
        assert!(!report.speedups.is_empty());
        assert!(
            render(&report, &metadata, &config(), None)
                .unwrap()
                .is_none()
        );
        let dir = tempfile::tempdir().unwrap();
        let issue_dir = dir.path().join("issue");
        assert!(!write(&issue_dir, &report, &metadata, &config(), None).unwrap());
        assert!(!issue_dir.exists());
    }
}
