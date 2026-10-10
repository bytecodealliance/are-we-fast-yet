mod common;

use common::{awfy, run_fake as run, run_root as root};

#[test]
fn run_writes_one_csv_per_unit() {
    let root = root();
    let out = run(root.path(), "b/");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let scratch = root.path().join("scratch");
    let mut files: Vec<String> = std::fs::read_dir(&scratch)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "a__benchmark.wasm.000.csv",
            "a__benchmark.wasm.001.csv",
            "a__benchmark.wasm.002.csv",
            "c__c.wasm.000.csv",
            "c__c.wasm.001.csv",
            "c__c.wasm.002.csv",
            "failures.toml",
        ]
    );

    let failures = std::fs::read_to_string(scratch.join("failures.toml")).unwrap();
    assert!(
        failures.starts_with("\"b/benchmark.wasm\" = \"process 0: exit status: 1: Error: benchmark b/benchmark.wasm failed on purpose; process 1:"),
        "{failures}"
    );

    let rows =
        awfy::runs::read_csv(std::fs::File::open(scratch.join("c__c.wasm.001.csv")).unwrap())
            .unwrap();
    assert_eq!(rows.len(), 6);
    assert!(
        rows.iter()
            .all(|m| m.wasm == "c/c.wasm" && m.engine.name == "wasmtime")
    );
    assert_eq!(
        rows[0].engine.flags.as_deref(),
        Some("--target x86_64-unknown-linux-gnu -Ccranelift-has-avx2")
    );
}

#[test]
fn run_fails_when_everything_fails() {
    let root = root();
    let out = run(root.path(), ".wasm");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("all 9 units failed"));
}

#[test]
fn run_needs_an_engine() {
    let root = root();
    let out = awfy(
        root.path(),
        &["run", "--engine", "/nonexistent.so", "--out", "scratch"],
    );
    assert!(!out.status.success());
}
