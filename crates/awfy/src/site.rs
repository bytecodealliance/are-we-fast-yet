//! `awfy site`: copy `site/` and generate the JSON the UI reads. The JSON is
//! the data contract between Rust and JS; see `docs/site.md`. Its snapshot
//! tests in `tests/snapshots/site/` pin the contract.

use crate::fingerprint::Fingerprint;
use crate::metadata::{Metadata, Timestamp};
use crate::regressions::{VIRTUAL_CYCLES, display};
use crate::runs::{DataDir, RunNumber};
use crate::slug::slug;
use anyhow::{Context, Result, bail};
use serde::{Serialize, Serializer};
use sightglass_analysis::geomean::{self, GEOMEAN};
use sightglass_analysis::summarize;
use sightglass_data::{Measurement, Phase};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The current schema version.
///
/// Bump on breaking changes.
const SCHEMA: u32 = 1;

/// Options for the `awfy site` command.
#[derive(clap::Args)]
pub struct Options {
    /// Output directory. Removed and recreated.
    #[arg(long)]
    out: PathBuf,
    /// Static files to copy instead of `<root>/site`.
    #[arg(long)]
    site: Option<PathBuf>,
    /// The `generated` timestamp, RFC 3339. Defaults to
    /// `$SOURCE_DATE_EPOCH`, then the current time.
    #[arg(long)]
    now: Option<Timestamp>,
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
struct Index {
    /// The schema version.
    schema: u32,
    /// When this index was generated.
    #[serde(serialize_with = "display")]
    generated: Timestamp,
    /// `owner/name` of this repository, for links to raw data.
    repo: String,
    /// The benchmark runs available.
    runs: Vec<IndexRun>,
    /// The benchmarks available.
    benchmarks: Vec<String>,
    /// The benchmark phases available.
    phases: Vec<String>,
    /// The benchmark metrics available.
    metrics: Vec<String>,
}

/// A summary of a benchmarking run inside the index file.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct IndexRun {
    /// The benchmarking run's number.
    run: RunNumber,
    /// The Wasmtime commit.
    wasmtime: String,
    /// The Wasmtime commit's date.
    #[serde(serialize_with = "display")]
    date: Timestamp,
    /// The fingerprint change for this run, if any.
    fingerprint_change: Option<String>,
}

/// One plotted point: a run, its central value, and its noise. See [`Stat`].
type Point = (RunNumber, Num, Num);

/// A benchmark's points by phase, then by metric, each in run order.
type Series = BTreeMap<String, BTreeMap<String, Vec<Point>>>;

/// The benchmark data series for a particular Wasm benchmark.
#[derive(Serialize)]
struct BenchmarkSeries {
    /// The Wasm benchmark.
    benchmark: String,
    /// A map `phase -> metric -> [run, center, noise]` points in run order.
    series: Series,
}

/// The data for a particular benchmark run for `run.html`.
///
/// This is `data/runs/NNNNN.json`.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct RunPage {
    /// The benchmark run number.
    run: RunNumber,
    /// The previous run, which changes are relative to.
    previous: Option<RunNumber>,
    /// The fingerprint changes, if any.
    fingerprint_change: Option<String>,
    /// `metadata.toml`, with timestamps as RFC 3339 strings.
    metadata: serde_json::Value,
    /// Virtual cycles for every (benchmark, phase) in this run, Geomean first.
    changes: Vec<RunChange>,
}

/// The summarized change for a particular run, compared to its previous
/// baseline.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct RunChange {
    /// The Wasm benchmark that changed.
    benchmark: String,
    /// The benchmark phase that changed.
    phase: String,
    /// This run's median count, or its geomean for [`GEOMEAN`].
    median: Num,
    /// How far this run's counts spread, as a percentage. See [`Stat::noise`].
    noise: Num,
    /// The previous run's median.
    previous: Option<Num>,
    /// The previous run's noise.
    previous_noise: Option<Num>,
    /// `(median - previous) / previous`.
    change: Option<Num>,
    /// How many benchmarks the geomean aggregated. See [`Stat::benchmarks`].
    benchmarks: Option<usize>,
}

/// A number written compactly: an integer when integral, otherwise rounded
/// to `digits` decimal places.
///
/// Only for use in the index and other summaries that get used in the UI. The
/// raw sightglass data should always be stored in full precision.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Num {
    /// The number value.
    value: f64,
    /// The number of digits to round to.
    digits: i32,
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

    /// A percentage. Four decimal places: real benchmarks spread anywhere from
    /// 0.06% to 7%, and two places would round most of them to zero.
    fn percent(value: f64) -> Self {
        Self { value, digits: 4 }
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

/// What the site plots for one (benchmark, phase, event) in one run: a
/// central value and how far the samples around it spread.
#[derive(Clone, Copy, Debug)]
struct Stat {
    /// The median count, or the geometric mean for [`GEOMEAN`].
    center: f64,
    /// How far the counts spread, as a percentage of `center`.
    ///
    /// For a benchmark this is `Summary::noise`: the interquartile range over
    /// the median, or the full range over it when there are too few samples to
    /// set outliers aside. For [`GEOMEAN`] it is `geomean_standard_error`,
    /// which is a different quantity; see there.
    noise: f64,
    /// How many benchmarks [`GEOMEAN`] aggregated, and `None` for an ordinary
    /// benchmark. A count that changes between runs means the geomean is
    /// summarizing different sets and its series is not comparable with
    /// itself, so the site shows it.
    benchmarks: Option<usize>,
}

/// One loaded visible run.
struct Run {
    number: RunNumber,
    metadata: Metadata,
    /// `(benchmark, phase, event)` to its statistics, Geomean included.
    stats: BTreeMap<(String, Phase, String), Stat>,
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
            let measurements = data.measurements(number)?;
            // A geomean over a different set of benchmarks is not comparable
            // with its neighbors.
            let with_geomean = metadata.failures.is_empty();
            Ok(Run {
                number,
                metadata,
                stats: stats(&measurements, with_geomean),
            })
        })
        .collect()
}

/// Summarize one run's measurements, adding a [`GEOMEAN`] row per
/// (phase, event) when `with_geomean`.
fn stats(
    measurements: &[Measurement<'_>],
    with_geomean: bool,
) -> BTreeMap<(String, Phase, String), Stat> {
    // A run measures one engine, so each summary is one (benchmark, phase,
    // event).
    let mut stats: BTreeMap<(String, Phase, String), Stat> = summarize::calculate(measurements)
        .into_iter()
        .map(|s| {
            (
                (s.wasm.to_string(), s.phase, s.event.to_string()),
                Stat {
                    center: s.median,
                    noise: s.noise(),
                    benchmarks: None,
                },
            )
        })
        .collect();

    if with_geomean {
        let errors = geomean_standard_errors(measurements);
        for g in geomean::summarize(measurements) {
            let key = (GEOMEAN.to_string(), g.phase, g.event.to_string());
            let noise = errors.get(&key).copied().unwrap_or(0.0);
            stats.insert(
                key,
                Stat {
                    center: g.geomean,
                    noise,
                    benchmarks: Some(g.benchmarks),
                },
            );
        }
    }

    stats
}

/// The standard error of each suite geomean, as a percentage, keyed like
/// [`Run::stats`].
///
/// The geomean aggregates the benchmarks' *mean* counts, so it has no samples
/// of its own and nothing that `Summary::noise` could measure. What it has
/// instead is how precisely those means pin it down, which is the standard
/// error of `ln(geomean)` by the delta method:
///
/// ```text
/// u_i = var(benchmark_i) / (n_i * mean(benchmark_i)^2)
/// standard_error = sqrt(sum(u_i)) / k
/// ```
///
/// This is the same quantity `sightglass_analysis::geomean` builds its
/// confidence interval from, for one engine rather than a comparison of two;
/// see that module's documentation. Note it is a standard error rather than a
/// spread, so unlike `Summary::noise` it shrinks as samples are added. The
/// site labels the two differently for that reason.
fn geomean_standard_errors(
    measurements: &[Measurement<'_>],
) -> BTreeMap<(String, Phase, String), f64> {
    // Only the benchmarks `geomean::summarize` aggregated contribute, so group
    // the counts the same way and then keep that set.
    let mut counts: BTreeMap<(Phase, String), BTreeMap<String, Vec<f64>>> = BTreeMap::new();
    for m in measurements {
        counts
            .entry((m.phase, m.event.to_string()))
            .or_default()
            .entry(m.wasm.to_string())
            .or_default()
            .push(m.count as f64);
    }

    let mut errors = BTreeMap::new();
    for ((phase, event), benchmarks) in counts {
        // A benchmark whose counts average zero has no logarithm, and
        // `geomean::summarize` leaves it out; leave it out of the error too.
        let usable: Vec<&Vec<f64>> = benchmarks
            .values()
            .filter(|c| !c.is_empty() && c.iter().sum::<f64>() > 0.0)
            .collect();
        if usable.len() < 2 {
            continue;
        }

        let mut u_total = 0.0;
        for samples in &usable {
            let n = samples.len() as f64;
            let mean = samples.iter().sum::<f64>() / n;
            // The sample variance, matching the significance tests. A single
            // sample has none.
            let var = if samples.len() < 2 {
                0.0
            } else {
                samples.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / (n - 1.0)
            };
            u_total += var / (n * mean * mean);
        }

        let k = usable.len() as f64;
        errors.insert(
            (GEOMEAN.to_string(), phase, event),
            u_total.sqrt() / k * 100.0,
        );
    }
    errors
}

/// Every generated file, as a path under `data/` and its JSON.
fn generate(runs: &[Run], repo: &str, now: Timestamp) -> Result<Vec<(PathBuf, String)>> {
    let mut benchmarks = BTreeSet::new();
    let mut phases = BTreeSet::new();
    let mut metrics = BTreeSet::new();
    for run in runs {
        for (b, p, e) in run.stats.keys() {
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
        let mut series = Series::new();
        for run in runs {
            for ((b, phase, event), stat) in &run.stats {
                if b == benchmark {
                    series
                        .entry(phase.to_string())
                        .or_default()
                        .entry(event.clone())
                        .or_default()
                        .push((
                            run.number,
                            Num::count(stat.center),
                            Num::percent(stat.noise),
                        ));
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
            .stats
            .iter()
            .filter(|((_, _, e), _)| e == VIRTUAL_CYCLES)
            .map(|((b, phase, e), stat)| {
                let before = previous
                    .and_then(|p| p.stats.get(&(b.clone(), *phase, e.clone())))
                    .copied();
                RunChange {
                    benchmark: b.clone(),
                    phase: phase.to_string(),
                    median: Num::count(stat.center),
                    noise: Num::percent(stat.noise),
                    previous: before.map(|b| Num::count(b.center)),
                    previous_noise: before.map(|b| Num::percent(b.noise)),
                    change: before.map(|b| Num::ratio((stat.center - b.center) / b.center)),
                    benchmarks: stat.benchmarks,
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
        let points = &files["benchmarks/pulldown-cmark-benchmark.json"]["series"]["execution"]["virtual-cycles"];
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
            stats: BTreeMap::new(),
        };
        for b in ["a/b.wasm", "a-b.wasm"] {
            run.stats.insert(
                (b.into(), Phase::Execution, VIRTUAL_CYCLES.into()),
                Stat {
                    center: 1.0,
                    noise: 0.0,
                    benchmarks: None,
                },
            );
        }
        assert!(generate(&[run], "a/b", now()).is_err());
    }
}
