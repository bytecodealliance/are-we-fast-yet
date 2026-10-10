mod common;

use common::{awfy_ok, fixture_root};

#[test]
fn regressions_writes_summary_and_issue() {
    let root = fixture_root();
    let summary = root.path().join("summary.md");
    std::fs::write(&summary, "earlier step\n").unwrap();
    let issue = root.path().join("issue");
    let args = |run: &str| {
        vec![
            "regressions".to_string(),
            "--run".into(),
            run.into(),
            "--summary".into(),
            summary.to_str().unwrap().into(),
            "--issue-dir".into(),
            issue.to_str().unwrap().into(),
        ]
    };
    let run = |run: &str| {
        let args = args(run);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        assert_eq!(awfy_ok(root.path(), &args), "");
    };

    // A steady pair: a summary, but no issue.
    run("2");
    assert!(!issue.exists());

    run("00003");
    let title = std::fs::read_to_string(issue.join("title.txt")).unwrap();
    assert_eq!(
        title,
        "Performance regression in 222222222222..333333333333 (are-we-fast-yet run 00003)\n"
    );
    let body = std::fs::read_to_string(issue.join("body.md")).unwrap();
    assert!(body.contains("benchmark.html?b=bz2-benchmark&metric=virtual-cycles"));

    let summary = std::fs::read_to_string(&summary).unwrap();
    assert!(summary.starts_with("earlier step\n## Regressions: run 00002 vs. 00001\n"));
    assert!(summary.contains("## Regressions: run 00003 vs. 00002\n"));
}

#[test]
fn issue_key() {
    let root = fixture_root();
    assert_eq!(
        awfy_ok(root.path(), &["issue-key", "--run", "42"]),
        "(are-we-fast-yet run 00042)\n"
    );
}
