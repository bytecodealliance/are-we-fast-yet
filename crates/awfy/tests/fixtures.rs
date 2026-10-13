//! The fixture `sightglass.csv.gz` files are generated from the plain CSVs in
//! `tests/fixtures/csv/`. See `tests/fixtures/README.md`.

mod common;

use awfy::runs::{CSV, read_csv, write_csv_gz};
use std::path::PathBuf;

/// `(source CSV, gzipped output)` for each fixture run.
fn pairs() -> Vec<(PathBuf, PathBuf)> {
    let fixtures = common::fixtures();
    let mut pairs: Vec<_> = std::fs::read_dir(fixtures.join("csv"))
        .unwrap()
        .map(|e| {
            let src = e.unwrap().path();
            let run = src.file_stem().unwrap().to_owned();
            let gz = fixtures.join("benchmark-data").join(run).join(CSV);
            (src, gz)
        })
        .collect();
    pairs.sort();
    pairs
}

fn gzip(src: &std::path::Path) -> Vec<u8> {
    let measurements = read_csv(std::fs::File::open(src).unwrap()).unwrap();
    let mut out = Vec::new();
    write_csv_gz(&measurements, &mut out).unwrap();
    out
}

/// Regenerate with `cargo test -p awfy --test fixtures -- --ignored`.
#[test]
#[ignore]
fn regenerate_fixtures() {
    for (src, gz) in pairs() {
        std::fs::write(&gz, gzip(&src)).unwrap();
    }
}

#[test]
fn fixtures_are_up_to_date() {
    let pairs = pairs();
    assert!(!pairs.is_empty());
    for (src, gz) in pairs {
        assert!(
            std::fs::read(&gz).ok() == Some(gzip(&src)),
            "{} is stale; run `cargo test -p awfy --test fixtures -- --ignored`",
            gz.display()
        );
    }
}

#[test]
fn fixtures_load() {
    let data = awfy::runs::DataDir::new(&common::fixtures());
    let runs = data.runs().unwrap();
    assert_eq!(runs.len(), 6);
    assert_eq!(data.visible_runs().unwrap().len(), 5);
    for run in runs {
        let metadata = data.metadata(run).unwrap();
        let measurements = data.measurements(run).unwrap();
        let benchmarks = if metadata.failures.is_empty() { 2 } else { 1 };
        // Benchmarks x phases x events x iterations.
        assert_eq!(measurements.len(), benchmarks * 3 * 3 * 3, "run {run}");
    }
}
