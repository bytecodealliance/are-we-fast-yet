mod common;

use anyhow::Result;
use awfy::metadata::Timestamp;
use awfy::record::{Options, SystemMethods, record};
use awfy::runs::{DataDir, RunNumber};
use std::path::Path;

struct FakeProbe;

impl SystemMethods for FakeProbe {
    fn command(&self, _cwd: &Path, program: &str, _args: &[&str]) -> Result<String> {
        Ok(match program {
            "git" => "f".repeat(40),
            "valgrind" => "valgrind-3.22.0".into(),
            "rustc" => "rustc 1.99.0 (b940084d7 2026-09-28)".into(),
            _ => anyhow::bail!("unexpected {program}"),
        })
    }
    fn now(&self) -> Timestamp {
        "2026-10-07T05:00:00Z".parse().unwrap()
    }
}

/// `awfy run` with the fake `sightglass-cli`, then `record` into a copy of the
/// fixtures. The new run loads back and round-trips.
#[test]
fn run_then_record() {
    let root = common::run_root();
    let out = common::run_fake(root.path(), "b/");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let options = options(vec![root.path().join("scratch")]);
    let run = record(root.path(), &options, &FakeProbe, None).unwrap();
    assert_eq!(run, RunNumber::new(7).unwrap());

    let data = DataDir::new(root.path());
    assert_eq!(data.runs().unwrap().last(), Some(&run));
    let metadata = data.metadata(run).unwrap();
    assert_eq!(
        awfy::metadata::Metadata::parse(&metadata.to_toml().unwrap()).unwrap(),
        metadata
    );
    assert_eq!(
        metadata.failures.keys().collect::<Vec<_>>(),
        ["b/benchmark.wasm"]
    );
    assert_ne!(metadata.environment.uname, "");

    let measurements = data.measurements(run).unwrap();
    // 2 benchmarks x 6 iterations x 3 phases x 2 events.
    assert_eq!(measurements.len(), 72);
    // Each surviving benchmark contributed all six of its iterations.
    let mut per_benchmark = std::collections::BTreeMap::new();
    for m in &measurements {
        *per_benchmark.entry(m.wasm.to_string()).or_insert(0) += 1;
    }
    assert_eq!(
        per_benchmark,
        [
            ("a/benchmark.wasm".to_string(), 36),
            ("c/c.wasm".to_string(), 36)
        ]
        .into()
    );
    assert!(measurements.iter().all(|m| m.engine.name == "wasmtime"));

    // Re-recording the same CSV gives byte-identical gzip.
    let again = record(root.path(), &options, &FakeProbe, None).unwrap();
    let csv = |r: RunNumber| std::fs::read(data.run_dir(r).join(awfy::runs::CSV)).unwrap();
    assert_eq!(csv(run), csv(again));
}

/// The same run split three ways records byte-identical data.
#[test]
fn sharded_run_records_the_same_data() {
    let root = common::run_root();
    let whole = common::run_fake(root.path(), "b/");
    assert!(whole.status.success());
    let mut shards = Vec::new();
    for k in 0..3 {
        let out = common::run_fake_shard(root.path(), "b/", &format!("{k}/3"));
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        shards.push(root.path().join(format!("scratch-{k}of3")));
    }
    let data = DataDir::new(root.path());
    let one = record(
        root.path(),
        &options(vec![root.path().join("scratch")]),
        &FakeProbe,
        None,
    )
    .unwrap();
    let three = record(root.path(), &options(shards), &FakeProbe, None).unwrap();
    let csv = |r: RunNumber| std::fs::read(data.run_dir(r).join(awfy::runs::CSV)).unwrap();
    assert_eq!(csv(one), csv(three));
    assert_eq!(
        data.metadata(one).unwrap().failures,
        data.metadata(three).unwrap().failures
    );
}

fn options(scratch: Vec<std::path::PathBuf>) -> Options {
    Options {
        scratch,
        wasmtime_commit: "7".repeat(40),
        wasmtime_commit_date: "2026-10-07T01:00:00Z".parse().unwrap(),
        started: "2026-10-07T03:17:00Z".parse().unwrap(),
        workflow_run: None,
        suite: None,
        iterations: None,
        wasmtime_repo: None,
    }
}
