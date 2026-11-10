mod common;

use awfy::bisect::{PLAN, Plan, Regressed};
use awfy::git::Candidate;
use awfy::runs::RunNumber;
use common::{awfy_ok, fake_sightglass_cli, run_root};
use sightglass_data::Phase;
use std::path::Path;

/// `awfy run --pair` with the fake `sightglass-cli` at counts of `base`.
fn measure(root: &Path, out: &Path, pairs: &[&str], base: u64) {
    let engine = root.join("libengine.so");
    std::fs::write(&engine, "").unwrap();
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_awfy"));
    cmd.arg("--root")
        .arg(root)
        .args(["run", "--iterations", "2", "--engine"])
        .arg(&engine)
        .arg("--sightglass-cli")
        .arg(fake_sightglass_cli())
        .arg("--out")
        .arg(out)
        .env("FAKE_SIGHTGLASS_BASE", base.to_string());
    for pair in pairs {
        cmd.args(["--pair", pair]);
    }
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Bisect the fixture's run 00003 over three candidates, where only the
/// middle one slows bz2's execution down, by as much as the run detected.
#[test]
fn bisect_blames_the_commit_that_reproduces() {
    let root = run_root();
    let candidate = |i: usize| Candidate {
        commit: format!("{:x}", 0xc0 + i).repeat(20),
        parent: format!("{:x}", 0xc1 + i).repeat(20),
        subject: format!("Change {i} (#{})", 15200 + i),
    };
    // The plan `bisect-plan` would write: the bz2 regression alone, so just
    // bz2's execution is measured.
    let plan = Plan {
        run: RunNumber::new(3).unwrap(),
        regressed: vec![Regressed {
            benchmark: "bz2/benchmark.wasm".into(),
            phase: Phase::Execution,
        }],
        pairs: vec!["execution:bz2/benchmark.wasm".into()],
        candidates: (0..3).map(candidate).collect(),
    };
    let plan_dir = root.path().join("plan");
    std::fs::create_dir(&plan_dir).unwrap();
    std::fs::write(plan_dir.join(PLAN), serde_json::to_string(&plan).unwrap()).unwrap();

    let results = root.path().join("results");
    let pairs: Vec<&str> = plan.pairs.iter().map(String::as_str).collect();
    for (i, slower) in [(0, false), (1, true), (2, false)] {
        let dir = results.join(format!("bisect-{}", candidate(i).commit));
        measure(root.path(), &dir.join("parent"), &pairs, 1_000_000);
        let base = if slower { 1_015_000 } else { 1_000_000 };
        measure(root.path(), &dir.join("candidate"), &pairs, base);
        // Only the execution phase was measured.
        let csvs: Vec<String> = std::fs::read_dir(dir.join("candidate"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|n| n.ends_with(".csv"))
            .collect();
        assert!(csvs.iter().all(|n| n.contains(".execution.")), "{csvs:?}");
    }

    let issue = root.path().join("issue");
    let summary = root.path().join("summary.md");
    awfy_ok(
        root.path(),
        &[
            "bisect-report",
            "--run",
            "3",
            "--plan",
            plan_dir.join(PLAN).to_str().unwrap(),
            "--results",
            results.to_str().unwrap(),
            "--summary",
            summary.to_str().unwrap(),
            "--issue-dir",
            issue.to_str().unwrap(),
        ],
    );
    let title = std::fs::read_to_string(issue.join("title.txt")).unwrap();
    assert_eq!(
        title,
        format!(
            "Performance regression in {} (#15201) (are-we-fast-yet run 00003)\n",
            &candidate(1).commit[..12]
        )
    );
    let summary = std::fs::read_to_string(summary).unwrap();
    assert!(
        summary.contains("Confirmed: filing the issue."),
        "{summary}"
    );
    assert!(
        summary.contains("| +1.50% | 100% | **blamed** |"),
        "{summary}"
    );

    // With no candidate slower, the detection is unconfirmed and nothing is
    // filed.
    std::fs::remove_dir_all(&issue).unwrap();
    let dir = results.join(format!("bisect-{}", candidate(1).commit));
    std::fs::remove_dir_all(dir.join("candidate")).unwrap();
    measure(root.path(), &dir.join("candidate"), &pairs, 1_000_000);
    let out = awfy_ok(
        root.path(),
        &[
            "bisect-report",
            "--run",
            "3",
            "--plan",
            plan_dir.join(PLAN).to_str().unwrap(),
            "--results",
            results.to_str().unwrap(),
            "--issue-dir",
            issue.to_str().unwrap(),
        ],
    );
    assert_eq!(out, "");
    assert!(!issue.exists());
}
