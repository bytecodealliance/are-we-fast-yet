mod common;

use common::awfy;
use std::path::{Path, PathBuf};

/// `fake-sightglass-cli`, an example that `cargo test` builds next to `awfy`.
/// (`cargo test --test run` alone does not; add `--examples`.)
pub fn fake_sightglass_cli() -> PathBuf {
    let awfy = Path::new(env!("CARGO_BIN_EXE_awfy"));
    let fake = awfy.parent().unwrap().join("examples").join(format!(
        "fake-sightglass-cli{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(fake.exists(), "{} was not built", fake.display());
    fake
}

/// A repo root with the fixture `config.toml` and a `sightglass/benchmarks/`
/// holding a suite of three (nonexistent) benchmarks.
fn root() -> tempfile::TempDir {
    let root = common::fixture_root();
    let benchmarks = root.path().join("sightglass/benchmarks");
    std::fs::create_dir_all(&benchmarks).unwrap();
    std::fs::write(
        benchmarks.join("default.suite"),
        "# Three benchmarks.\na/benchmark.wasm\nb/benchmark.wasm\nc/c.wasm\n",
    )
    .unwrap();
    root
}

fn run(root: &Path, fail: &str) -> std::process::Output {
    let engine = root.join("libengine.so");
    std::fs::write(&engine, "").unwrap();
    let out = root.join("scratch");
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_awfy"));
    cmd.arg("--root")
        .arg(root)
        .args(["run", "--jobs", "3", "--engine"])
        .arg(&engine)
        .arg("--sightglass-cli")
        .arg(fake_sightglass_cli())
        .arg("--out")
        .arg(&out)
        .env("FAKE_SIGHTGLASS_FAIL", fail);
    cmd.output().unwrap()
}

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
