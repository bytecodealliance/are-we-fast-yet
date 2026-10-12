//! `awfy site`: copy `site/` and generate the JSON the UI reads. The JSON is
//! the data contract between Rust and JS; see `docs/site.md`. Its snapshot
//! tests in `tests/snapshots/site/` pin the contract.

use crate::fingerprint::Fingerprint;
use crate::metadata::{Metadata, Timestamp};
use crate::regressions::{display, VIRTUAL_CYCLES};
use crate::runs::{DataDir, RunNumber};
use crate::slug::slug;
use anyhow::{bail, Context, Result};
use serde::{Serialize, Serializer};
use sightglass_analysis::geomean::{self, GEOMEAN};
use sightglass_data::{Measurement, Phase};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The current schema version.
///
/// Bump on breaking changes.
pub const SCHEMA: u32 = 1;

/// Options for the `awfy site` command.
#[derive(clap::Args)]
pub struct Options {
    /// Output directory. Removed and recreated.
    #[arg(long)]
    pub out: PathBuf,
    /// Static files to copy instead of `<root>/site`.
    #[arg(long)]
    pub site: Option<PathBuf>,
    /// The `generated` timestamp, RFC 3339. Defaults to
    /// `$SOURCE_DATE_EPOCH`, then the current time.
    #[arg(long)]
    pub now: Option<Timestamp>,
}

/// Top-level entries in `site/` that are for development, not deployment.
const NOT_DEPLOYED: &[&str] = &[
    "node_modules",
    "package.json",
    "package-lock.json",
    "playwright.config.js",
    "tests",
];

/// The index file summarizing the benchmark data available.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct Index {
    /// The schema version.
    pub schema: u32,
    /// When this index was generated.
    #[serde(serialize_with = "display")]
    pub generated: Timestamp,
    /// `owner/name` of this repository, for links to raw data.
    pub repo: String,
    /// The benchmark runs available.
    pub runs: Vec<IndexRun>,
    /// The benchmarks available.
    pub benchmarks: Vec<String>,
    /// The benchmark phases available.
    pub phases: Vec<String>,
    /// The benchmark metrics available.
    pub metrics: Vec<String>,
}

/// A summary of a benchmarking run inside the index file.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct IndexRun {
    /// The benchmarking run's number.
    pub run: RunNumber,
    /// The Wasmtime commit.
    pub wasmtime: String,
    /// The Wasmtime commit's date.
    #[serde(serialize_with = "display")]
    pub date: Timestamp,
    /// The fingerprint change for this run, if any.
    pub fingerprint_change: Option<String>,
}

/// The benchmark data series for a particular Wasm benchmark.
#[derive(Serialize)]
pub struct BenchmarkSeries {
    /// The Wasm benchmark.
    pub benchmark: String,
    /// A map `phase -> metric -> [run, mean]` points in run order.
    pub series: BTreeMap<String, BTreeMap<String, Vec<(RunNumber, Num)>>>,
}

/// The data for a particular benchmark run for `run.html`.
///
/// This is `data/runs/NNNNN.json`.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct RunPage {
    /// The benchmark run number.
    pub run: RunNumber,
    /// The previous run, which changes are relative to.
    pub previous: Option<RunNumber>,
    /// The fingerprint changes, if any.
    pub fingerprint_change: Option<String>,
    /// `metadata.toml`, with timestamps as RFC 3339 strings.
    pub metadata: serde_json::Value,
    /// Virtual cycles for every (benchmark, phase) in this run, Geomean first.
    pub changes: Vec<RunChange>,
}

/// The summarized change for a particular run, compared to its previous
/// baseline.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct RunChange {
    /// The Wasm benchmark that changed.
    pub benchmark: String,
    /// The benchmark phase that changed.
    pub phase: String,
    /// The mean of this run, after the change.
    pub mean: Num,
    /// The mean of the previous run.
    pub previous: Option<Num>,
    /// `(mean - previous) / previous`.
    pub change: Option<Num>,
}

/// A number written compactly: an integer when integral, otherwise rounded
/// to `digits` decimal places.
///
/// Only for use in the index and other summaries that get used in the UI. The
/// raw sightglass data should always be stored in full precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Num {
    /// The number value.
    pub value: f64,
    /// The number of digits to round to.
    pub digits: i32,
}

impl Num {
    /// A mean count. Two decimal places is far below callgrind's noise, even
    /// for counts in the billions.
    fn count(value: f64) -> Self {
        Self { value, digits: 2 }
    }

    fn ratio(value: f64) -> Self {
        Self { value, digits: 6 }
    }
}

impl Serialize for Num {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let scale = 10f64.powi(self.digits);
        let v = (self.value * scale).round() / scale;
        if v.fract() == 0.0 && v.abs() < 2f64.powi(53) {
            s.serialize_i64(v as i64)
        } else {
            s.serialize_f64(v)
        }
    }
}

/// One loaded visible run.
struct Run {
    number: RunNumber,
    metadata: Metadata,
    /// `(benchmark, phase, event)` to mean count, Geomean included.
    means: BTreeMap<(String, Phase, String), f64>,
}

/// Run `awfy site` with the given options.
pub fn run(root: &Path, options: Options) -> Result<()> {
    let site = options.site.clone().unwrap_or_else(|| root.join("site"));
    let now = match options.now {
        Some(now) => now,
        None => match std::env::var("SOURCE_DATE_EPOCH") {
            Ok(epoch) => Timestamp(jiff::Timestamp::from_second(
                epoch.parse().context("parsing SOURCE_DATE_EPOCH")?,
            )?),
            Err(_) => Timestamp::now(),
        },
    };
    let config = crate::config::Config::load(root)?;
    build(&DataDir::new(root), &config, &site, &options.out, now)
}

fn build(
    data: &DataDir,
    config: &crate::config::Config,
    site: &Path,
    out: &Path,
    now: Timestamp,
) -> Result<()> {
    if !site.is_dir() {
        bail!("site directory {} does not exist", site.display());
    }
    match std::fs::remove_dir_all(out) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(e).with_context(|| format!("removing {}", out.display()));
        }
        _ => {}
    }
    copy_dir(site, out, true)?;

    let runs = load(data)?;
    let files = generate(&runs, &config.site.repo, now)?;
    let mut bytes = 0;
    for (path, json) in &files {
        let path = out.join("data").join(path);
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, json).with_context(|| format!("writing {}", path.display()))?;
        bytes += json.len();
    }
    log::info!(
        "wrote {} JSON files ({} KiB) for {} runs to {}",
        files.len(),
        bytes / 1024,
        runs.len(),
        out.join("data").display()
    );
    Ok(())
}

fn copy_dir(from: &Path, to: &Path, top: bool) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || (top && NOT_DEPLOYED.contains(&name_str.as_ref())) {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target, false)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn load(data: &DataDir) -> Result<Vec<Run>> {
    data.visible_runs()?
        .into_iter()
        .map(|number| {
            let metadata = data.metadata(number)?;
            let mut measurements = data.measurements(number)?;
            // A geomean over a different set of benchmarks is not comparable
            // with its neighbors.
            if metadata.failures.is_empty() {
                geomean::add(&mut measurements);
            }
            Ok(Run {
                number,
                metadata,
                means: means(&measurements),
            })
        })
        .collect()
}

fn means(measurements: &[Measurement<'_>]) -> BTreeMap<(String, Phase, String), f64> {
    let mut sums: BTreeMap<(String, Phase, String), (u128, u64)> = BTreeMap::new();
    for m in measurements {
        let (sum, n) = sums
            .entry((m.wasm.to_string(), m.phase, m.event.to_string()))
            .or_default();
        *sum += u128::from(m.count);
        *n += 1;
    }
    sums.into_iter()
        .map(|(k, (sum, n))| (k, sum as f64 / n as f64))
        .collect()
}

/// Every generated file, as a path under `data/` and its JSON.
fn generate(runs: &[Run], repo: &str, now: Timestamp) -> Result<Vec<(PathBuf, String)>> {
    let mut benchmarks = BTreeSet::new();
    let mut phases = BTreeSet::new();
    let mut metrics = BTreeSet::new();
    for run in runs {
        for (b, p, e) in run.means.keys() {
            benchmarks.insert(b.clone());
            phases.insert(*p);
            metrics.insert(e.clone());
        }
    }
    // Geomean first, then the rest sorted.
    let has_geomean = benchmarks.remove(GEOMEAN);
    let benchmarks: Vec<String> = has_geomean
        .then(|| GEOMEAN.to_string())
        .into_iter()
        .chain(benchmarks)
        .collect();
    let has_cycles = metrics.remove(VIRTUAL_CYCLES);
    let metrics: Vec<String> = has_cycles
        .then(|| VIRTUAL_CYCLES.to_string())
        .into_iter()
        .chain(metrics)
        .collect();

    let mut slugs = BTreeMap::new();
    for b in &benchmarks {
        if let Some(other) = slugs.insert(slug(b), b) {
            bail!("benchmarks `{other}` and `{b}` have the same slug");
        }
    }

    let fingerprint_changes: Vec<Option<String>> = runs
        .iter()
        .enumerate()
        .map(|(i, run)| {
            let previous = i.checked_sub(1).map(|i| &runs[i])?;
            let changes = Fingerprint::of(&previous.metadata).diff(&Fingerprint::of(&run.metadata));
            (!changes.is_empty()).then(|| changes.join(", "))
        })
        .collect();

    let mut files = Vec::new();
    let index = Index {
        schema: SCHEMA,
        generated: now,
        repo: repo.to_string(),
        runs: runs
            .iter()
            .zip(&fingerprint_changes)
            .map(|(run, change)| IndexRun {
                run: run.number,
                wasmtime: run.metadata.wasmtime.commit.clone(),
                date: run.metadata.wasmtime.commit_date,
                fingerprint_change: change.clone(),
            })
            .collect(),
        benchmarks: benchmarks.clone(),
        phases: phases.iter().map(Phase::to_string).collect(),
        metrics,
    };
    files.push(("index.json".into(), serde_json::to_string(&index)?));

    for benchmark in &benchmarks {
        let mut series: BTreeMap<String, BTreeMap<String, Vec<(RunNumber, Num)>>> = BTreeMap::new();
        for run in runs {
            for ((b, phase, event), &mean) in &run.means {
                if b == benchmark {
                    series
                        .entry(phase.to_string())
                        .or_default()
                        .entry(event.clone())
                        .or_default()
                        .push((run.number, Num::count(mean)));
                }
            }
        }
        let json = serde_json::to_string(&BenchmarkSeries {
            benchmark: benchmark.clone(),
            series,
        })?;
        files.push((format!("benchmarks/{}.json", slug(benchmark)).into(), json));
    }

    for (i, run) in runs.iter().enumerate() {
        let previous = i.checked_sub(1).map(|i| &runs[i]);
        let mut changes: Vec<RunChange> = run
            .means
            .iter()
            .filter(|((_, _, e), _)| e == VIRTUAL_CYCLES)
            .map(|((b, phase, e), &mean)| {
                let before = previous
                    .and_then(|p| p.means.get(&(b.clone(), *phase, e.clone())))
                    .copied();
                RunChange {
                    benchmark: b.clone(),
                    phase: phase.to_string(),
                    mean: Num::count(mean),
                    previous: before.map(Num::count),
                    change: before.map(|before| Num::ratio((mean - before) / before)),
                }
            })
            .collect();
        changes.sort_by_key(|c| c.benchmark != GEOMEAN);
        let page = RunPage {
            run: run.number,
            previous: previous.map(|p| p.number),
            fingerprint_change: fingerprint_changes[i].clone(),
            metadata: metadata_json(&run.metadata)?,
            changes,
        };
        files.push((
            format!("runs/{}.json", run.number).into(),
            serde_json::to_string(&page)?,
        ));
    }
    Ok(files)
}

/// `metadata.toml` as JSON. Via TOML text, since `toml`'s datetimes serialize
/// as special tables anywhere else.
fn metadata_json(metadata: &Metadata) -> Result<serde_json::Value> {
    fn convert(v: toml::Value) -> serde_json::Value {
        use serde_json::Value as J;
        match v {
            toml::Value::String(s) => J::String(s),
            toml::Value::Integer(i) => J::from(i),
            toml::Value::Float(f) => J::from(f),
            toml::Value::Boolean(b) => J::Bool(b),
            toml::Value::Datetime(d) => J::String(d.to_string()),
            toml::Value::Array(a) => J::Array(a.into_iter().map(convert).collect()),
            toml::Value::Table(t) => {
                J::Object(t.into_iter().map(|(k, v)| (k, convert(v))).collect())
            }
        }
    }
    let table: toml::Table = metadata.to_toml()?.parse()?;
    Ok(convert(toml::Value::Table(table)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::assert_snapshot;

    fn fixtures() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    fn now() -> Timestamp {
        "2026-10-10T04:30:00Z".parse().unwrap()
    }

    fn files() -> BTreeMap<String, serde_json::Value> {
        let runs = load(&DataDir::new(&fixtures())).unwrap();
        generate(&runs, "bytecodealliance/are-we-fast-yet", now())
            .unwrap()
            .into_iter()
            .map(|(p, json)| {
                (
                    p.to_str().unwrap().to_string(),
                    serde_json::from_str(&json).unwrap(),
                )
            })
            .collect()
    }

    /// The Rust/JS data contract. If these change, update `site/js/` too.
    #[test]
    fn snapshots() {
        let files = files();
        for (path, name) in [
            ("index.json", "index"),
            ("benchmarks/bz2-benchmark.json", "benchmark-bz2"),
            ("benchmarks/Geomean.json", "benchmark-geomean"),
            ("runs/00003.json", "run-00003"),
        ] {
            assert_snapshot(
                &format!("site/{name}.json"),
                &crate::snapshot::json(&files[path]),
            );
        }
    }

    #[test]
    fn file_list() {
        let files = files();
        let paths: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "benchmarks/Geomean.json",
                "benchmarks/bz2-benchmark.json",
                "benchmarks/pulldown-cmark-benchmark.json",
                "index.json",
                "runs/00001.json",
                "runs/00002.json",
                "runs/00003.json",
                "runs/00005.json",
                "runs/00006.json",
            ]
        );
    }

    #[test]
    fn excluded_runs_are_absent() {
        let files = files();
        let runs: Vec<u64> = files["index.json"]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["run"].as_u64().unwrap())
            .collect();
        assert_eq!(runs, [1, 2, 3, 5, 6]);
        let points =
            &files["benchmarks/bz2-benchmark.json"]["series"]["execution"]["virtual-cycles"];
        let runs: Vec<u64> = points
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p[0].as_u64().unwrap())
            .collect();
        assert_eq!(runs, [1, 2, 3, 5, 6]);
    }

    #[test]
    fn fingerprint_change_is_labeled() {
        let files = files();
        let changes: Vec<&serde_json::Value> = files["index.json"]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| &r["fingerprint-change"])
            .collect();
        assert_eq!(
            changes,
            [
                &serde_json::Value::Null,
                &serde_json::Value::Null,
                &serde_json::Value::Null,
                &serde_json::json!("valgrind 3.22.0 -> 3.24.0"),
                &serde_json::Value::Null,
            ]
        );
    }

    #[test]
    fn no_geomean_for_failed_runs() {
        let files = files();
        let points = &files["benchmarks/Geomean.json"]["series"]["execution"]["virtual-cycles"];
        let runs: Vec<u64> = points
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p[0].as_u64().unwrap())
            .collect();
        assert_eq!(runs, [1, 2, 3, 5]);
        // pulldown-cmark is missing from 00006, so has no point there.
        let points = &files["benchmarks/pulldown-cmark-benchmark.json"]["series"]["execution"]
            ["virtual-cycles"];
        assert_eq!(points.as_array().unwrap().len(), 4);
    }

    #[test]
    fn numbers() {
        let json = |n: Num| serde_json::to_string(&n).unwrap();
        assert_eq!(json(Num::count(1000000.0)), "1000000");
        assert_eq!(json(Num::count(1234567890.3333)), "1234567890.33");
        assert_eq!(json(Num::count(12345678901234.5)), "12345678901234.5");
        assert_eq!(json(Num::ratio(0.0150000001)), "0.015");
        assert_eq!(json(Num::ratio(0.0)), "0");
    }

    #[test]
    fn slug_collisions_are_errors() {
        let mut run = Run {
            number: RunNumber::new(1).unwrap(),
            metadata: crate::metadata::tests::example(),
            means: BTreeMap::new(),
        };
        for b in ["a/b.wasm", "a-b.wasm"] {
            run.means
                .insert((b.into(), Phase::Execution, VIRTUAL_CYCLES.into()), 1.0);
        }
        assert!(generate(&[run], "a/b", now()).is_err());
    }
}
