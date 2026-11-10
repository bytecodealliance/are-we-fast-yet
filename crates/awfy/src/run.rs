//! `awfy run`: run the suite under callgrind as many small, independent
//! `sightglass-cli` invocations, `--jobs` at a time. Each (benchmark,
//! iteration index) unit writes its own CSV to the output directory, longest
//! benchmark first as estimated from `benchmark-data/`. With `--shard`, only
//! that shard's iterations run, so several hosts can split a run. With
//! `--pair`, only the given (benchmark, phase) pairs run, each measuring just
//! that phase, as a bisect needs. See `docs/architecture.md`.

use crate::config::Config;
use crate::runs::DataDir;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sightglass_data::{Measurement, Phase};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The shard manifest's file name, written by `awfy run` beside its CSVs and
/// read by `awfy record`.
pub const MANIFEST: &str = "shard.toml";

/// One shard of a run: every iteration `i` with `i % count == index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shard {
    /// Which shard this is, from 0.
    pub index: u32,
    /// How many shards the run is split into.
    pub count: u32,
}

impl Shard {
    /// The whole run, unsharded.
    pub const ALL: Self = Self { index: 0, count: 1 };

    /// Does this shard run the given iteration?
    pub fn runs(&self, iteration: u32) -> bool {
        iteration % self.count == self.index
    }
}

impl fmt::Display for Shard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.index, self.count)
    }
}

impl std::str::FromStr for Shard {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (index, count) = s
            .split_once('/')
            .with_context(|| format!("invalid shard `{s}`; expected e.g. `0/3`"))?;
        let shard = Self {
            index: index
                .parse()
                .with_context(|| format!("invalid shard `{s}`"))?,
            count: count
                .parse()
                .with_context(|| format!("invalid shard `{s}`"))?,
        };
        anyhow::ensure!(
            shard.index < shard.count,
            "invalid shard `{s}`; the index must be less than the count"
        );
        Ok(shard)
    }
}

/// `shard.toml`: what one `awfy run` measured, where, and what failed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Manifest {
    /// See `Shard::index`.
    pub index: u32,
    /// See `Shard::count`.
    pub count: u32,
    /// The host that ran this shard.
    pub host: Host,
    /// Every unit that failed, by benchmark and then iteration.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<Failed>,
}

/// A host that ran a shard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Host {
    /// `uname -a`.
    pub uname: String,
    /// The CPU model name.
    pub cpu: String,
    /// The CPU's feature flags. Valgrind passes some through to the guest,
    /// so they could steer what code runs.
    pub cpu_flags: String,
}

impl Host {
    /// Probe the current host.
    fn probe() -> Result<Self> {
        let output = std::process::Command::new("uname")
            .arg("-a")
            .output()
            .context("running `uname -a`")?;
        anyhow::ensure!(
            output.status.success(),
            "`uname -a` failed: {}",
            output.status
        );
        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        Ok(Self {
            uname: String::from_utf8(output.stdout)?.trim().to_string(),
            cpu: cpuinfo_field(&cpuinfo, "model name").unwrap_or_else(|| "unknown".into()),
            cpu_flags: cpuinfo_field(&cpuinfo, "flags").unwrap_or_else(|| "unknown".into()),
        })
    }
}

/// The first value of `field` in `/proc/cpuinfo`'s text.
fn cpuinfo_field(cpuinfo: &str, field: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == field).then(|| value.trim().to_string())
    })
}

/// A unit that failed.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Failed {
    /// The benchmark, as `Unit::wasm`.
    pub wasm: String,
    /// The iteration that failed.
    pub iteration: u32,
    /// The one phase the unit measured, or `None` for every phase.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    /// A short reason.
    pub reason: String,
}

/// A benchmark measured in just one phase, written `phase:wasm`, e.g.
/// `execution:bz2/benchmark.wasm`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pair {
    /// The phase to measure.
    pub phase: Phase,
    /// The benchmark, relative to `sightglass/benchmarks/`.
    pub wasm: String,
}

impl fmt::Display for Pair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.phase, self.wasm)
    }
}

impl std::str::FromStr for Pair {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        let (phase, wasm) = s.split_once(':').with_context(|| {
            format!("invalid pair `{s}`; expected e.g. `execution:bz2/benchmark.wasm`")
        })?;
        anyhow::ensure!(!wasm.is_empty(), "invalid pair `{s}`: no benchmark");
        Ok(Self {
            phase: phase
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid pair `{s}`: {e}"))?,
            wasm: wasm.to_string(),
        })
    }
}

/// Options for the `awfy run` command.
#[derive(clap::Args)]
pub struct Options {
    /// Path to the Wasmtime `libengine.so`.
    #[arg(long)]
    engine: PathBuf,
    /// Path to `sightglass-cli`.
    #[arg(long, default_value = "sightglass-cli")]
    sightglass_cli: PathBuf,
    /// Scratch directory for per-unit CSVs. Must be empty or missing.
    #[arg(long)]
    out: PathBuf,
    /// Suite to run instead of `config.toml`'s, relative to `sightglass/`.
    #[arg(long)]
    suite: Option<PathBuf>,
    /// Run just this benchmark, measuring just this phase, instead of a
    /// suite, e.g. `execution:bz2/benchmark.wasm`. Repeatable.
    #[arg(long = "pair", conflicts_with = "suite")]
    pairs: Vec<Pair>,
    /// Iterations per benchmark instead of `config.toml`'s. Recorded in
    /// metadata by `awfy record --iterations`.
    #[arg(long)]
    iterations: Option<u32>,
    /// Units to run at once. Defaults to the available parallelism.
    #[arg(long)]
    jobs: Option<usize>,
    /// Run only this shard's iterations, e.g. `1/3` for iterations 1, 4, ....
    #[arg(long, default_value_t = Shard::ALL)]
    shard: Shard,
}

/// What to measure: a benchmark, in every phase or just one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    /// Relative to `sightglass/benchmarks/`, or absolute if outside it.
    wasm: PathBuf,
    /// The one phase to measure, or `None` for every phase.
    phase: Option<Phase>,
}

/// One `sightglass-cli` invocation: one iteration of a target.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Unit {
    /// What to measure.
    target: Target,
    /// Which iteration of its target this unit is.
    iteration: u32,
}

impl Unit {
    /// The output file name.
    fn csv_name(&self) -> String {
        let wasm = self.target.wasm.to_string_lossy();
        let wasm = wasm.trim_start_matches('/').replace('/', "__");
        let phase = match self.target.phase {
            Some(phase) => format!(".{phase}"),
            None => String::new(),
        };
        // The zero-padded iteration index keeps a benchmark's files in
        // iteration order when sorted.
        format!("{wasm}{phase}.{:03}.csv", self.iteration)
    }
}

/// How a process exited.
#[derive(Clone, Debug)]
struct Exit {
    /// Did the process exit successfully?
    success: bool,
    /// E.g. `exit status: 1`.
    status: String,
    /// The `stderr` of this process.
    stderr: String,
}

/// A trait for spawning `sightglass-cli` subprocesses, so we can fake it in
/// tests.
trait Runner: Sync {
    /// Run `sightglass-cli`.
    fn run(&self, program: &Path, args: &[OsString], cwd: &Path) -> Result<Exit>;
}

/// The real `sightglass-cli` runner.
struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, program: &Path, args: &[OsString], cwd: &Path) -> Result<Exit> {
        let output = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()
            .with_context(|| format!("spawning {}", program.display()))?;
        Ok(Exit {
            success: output.status.success(),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Run `awfy run` with the given options.
pub fn run(root: &Path, options: Options) -> Result<()> {
    let mut config = Config::load(root)?;
    if let Some(iterations) = options.iterations {
        config.benchmark.iterations = iterations;
    }
    let manifest = run_with(root, &config, &options, &ProcessRunner, Host::probe()?)?;
    if !manifest.failed.is_empty() {
        log::warn!("{} unit(s) failed", manifest.failed.len());
    }
    Ok(())
}

/// Run every unit in the shard and write its manifest. Errors only on setup
/// problems or if every unit failed.
fn run_with(
    root: &Path,
    config: &Config,
    options: &Options,
    runner: &dyn Runner,
    host: Host,
) -> Result<Manifest> {
    let jobs = match options.jobs {
        Some(jobs) => jobs,
        None => std::thread::available_parallelism()?.get(),
    };
    anyhow::ensure!(jobs > 0, "--jobs must be at least 1");
    let root = std::path::absolute(root)?;
    let benchmarks_dir = root.join("sightglass/benchmarks");
    let targets: Vec<Target> = if options.pairs.is_empty() {
        // Lexically, so `..` means the same even if `sightglass` is a symlink.
        let suite = normalize(
            &root.join("sightglass").join(
                options
                    .suite
                    .as_deref()
                    .unwrap_or(config.benchmark.suite.as_ref()),
            ),
        );
        let wasms = parse_suite(&suite, &benchmarks_dir)?;
        anyhow::ensure!(!wasms.is_empty(), "{} lists no benchmarks", suite.display());
        wasms
            .into_iter()
            .map(|wasm| Target { wasm, phase: None })
            .collect()
    } else {
        options
            .pairs
            .iter()
            .map(|p| Target {
                wasm: p.wasm.clone().into(),
                phase: Some(p.phase),
            })
            .collect()
    };

    std::fs::create_dir_all(&options.out)
        .with_context(|| format!("creating {}", options.out.display()))?;
    let out = options.out.canonicalize()?;
    if std::fs::read_dir(&out)?.next().is_some() {
        bail!("{} is not empty", out.display());
    }
    let engine = options
        .engine
        .canonicalize()
        .with_context(|| format!("engine {}", options.engine.display()))?;
    // A relative program path with a separator would resolve against the
    // child's working directory, so make it absolute. A bare name is looked up
    // on `PATH`.
    let sightglass_cli = if options.sightglass_cli.components().count() > 1 {
        options
            .sightglass_cli
            .canonicalize()
            .with_context(|| format!("sightglass-cli {}", options.sightglass_cli.display()))?
    } else {
        options.sightglass_cli.clone()
    };

    let wasms: Vec<PathBuf> = targets.iter().map(|t| t.wasm.clone()).collect();
    let estimates = estimates(&DataDir::new(&root), &wasms).unwrap_or_else(|e| {
        log::warn!("running in suite order; could not estimate benchmark costs: {e:#}");
        Estimates::new()
    });
    log::info!(
        "estimated the cost of {} of {} benchmarks from recorded runs",
        estimates.len(),
        wasms.len()
    );
    let units = schedule(
        &targets,
        config.benchmark.iterations,
        options.shard,
        &estimates,
    );
    anyhow::ensure!(
        !units.is_empty(),
        "shard {} has no iterations of {}",
        options.shard,
        config.benchmark.iterations
    );

    let total = units.len();
    log::info!(
        "running {total} units ({} targets x {} iterations, shard {}), {} at a time",
        targets.len(),
        config.benchmark.iterations,
        options.shard,
        jobs
    );

    let queue = Mutex::new(units.into_iter().enumerate().collect::<VecDeque<_>>());
    let done = Mutex::new(0usize);
    let failed: Mutex<Vec<(Unit, String)>> = Mutex::new(Vec::new());
    let start = Instant::now();

    let worker = || -> Result<()> {
        loop {
            let Some((i, unit)) = queue.lock().unwrap().pop_front() else {
                return Ok(());
            };
            let phase = match unit.target.phase {
                Some(phase) => format!(" {phase}"),
                None => String::new(),
            };
            let name = format!(
                "{}{phase} iteration {}",
                unit.target.wasm.display(),
                unit.iteration
            );
            log::info!("[{}/{total}] started {name}", i + 1);
            let unit_start = Instant::now();
            let csv = out.join(unit.csv_name());
            let args = command_line(config, &engine, &csv, &unit);
            let result = runner
                .run(&sightglass_cli, &args, &benchmarks_dir)
                .and_then(|exit| check(&exit, &csv));
            let elapsed = format_duration(unit_start.elapsed());
            let n = {
                let mut done = done.lock().unwrap();
                *done += 1;
                *done
            };
            match result {
                Ok(()) => log::info!(
                    "[{n}/{total} done, {} elapsed] finished {name} in {elapsed}",
                    format_duration(start.elapsed())
                ),
                Err(e) => {
                    log::error!("[{n}/{total} done] {name} failed after {elapsed}: {e:#}");
                    failed.lock().unwrap().push((unit, short_reason(&e)));
                }
            }
        }
    };
    std::thread::scope(|s| {
        let workers: Vec<_> = (0..jobs.min(total)).map(|_| s.spawn(worker)).collect();
        workers
            .into_iter()
            .try_for_each(|w| w.join().expect("worker panicked"))
    })?;

    let failed = failed.into_inner().unwrap();
    if failed.len() == total {
        bail!("all {total} units failed");
    }
    let mut failed: Vec<Failed> = failed
        .into_iter()
        .map(|(unit, reason)| Failed {
            wasm: unit.target.wasm.to_string_lossy().into_owned(),
            iteration: unit.iteration,
            phase: unit.target.phase,
            reason,
        })
        .collect();
    failed.sort();
    let manifest = Manifest {
        index: options.shard.index,
        count: options.shard.count,
        host,
        failed,
    };
    std::fs::write(out.join(MANIFEST), toml::to_string(&manifest)?)?;
    log::info!(
        "finished {total} units in {}",
        format_duration(start.elapsed())
    );
    Ok(manifest)
}

/// The `sightglass-cli` arguments for one unit, run from
/// `sightglass/benchmarks/` so that the CSV's `wasm` column is relative to it.
fn command_line(config: &Config, engine: &Path, csv: &Path, unit: &Unit) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "benchmark",
        "--measure",
        "callgrind",
        "--iters",
        "1",
        "--raw",
        "--output-format",
        "csv",
    ]
    .map(OsString::from)
    .to_vec();
    args.push("--output-file".into());
    args.push(csv.into());
    args.push("--engine".into());
    args.push(engine.into());
    args.push("--name".into());
    args.push("wasmtime".into());
    // `=`, since the flags start with `-`.
    args.push(format!("--engine-flags-common={}", config.benchmark.engine_flags).into());
    if let Some(phase) = unit.target.phase {
        // Sightglass still runs the earlier phases, unmeasured, and writes
        // only this one's rows.
        args.push("--benchmark-phase".into());
        args.push(phase.to_string().into());
    }
    args.push("--".into());
    args.push(unit.target.wasm.clone().into());
    args
}

/// Turn a finished process into success, or an error with a short reason.
fn check(exit: &Exit, csv: &Path) -> Result<()> {
    if !exit.success {
        let tail: Vec<&str> = exit.stderr.lines().rev().take(20).collect();
        for line in tail.iter().rev() {
            log::error!("  | {line}");
        }
        let last = tail.iter().find(|l| !l.trim().is_empty());
        match last {
            Some(last) => bail!("{}: {}", exit.status, last.trim()),
            None => bail!("{}", exit.status),
        }
    }
    let file = std::fs::File::open(csv).context("no output file")?;
    let rows = crate::runs::read_csv(file).context("unreadable output file")?;
    anyhow::ensure!(!rows.is_empty(), "empty output file");
    Ok(())
}

fn short_reason(e: &anyhow::Error) -> String {
    const MAX: usize = 200;
    let reason = format!("{e:#}");
    match reason.char_indices().nth(MAX) {
        Some((i, _)) => format!("{}...", &reason[..i]),
        None => reason,
    }
}

/// Every unit in `shard`, longest first, so that a long one doesn't start
/// last and stretch the run out behind it.
fn schedule(targets: &[Target], iterations: u32, shard: Shard, estimates: &Estimates) -> Vec<Unit> {
    let mut units: Vec<Unit> = targets
        .iter()
        .flat_map(|target| {
            (0..iterations)
                .filter(|&i| shard.runs(i))
                .map(|iteration| Unit {
                    target: target.clone(),
                    iteration,
                })
        })
        .collect();
    // An unestimated benchmark could be the longest of all, so schedule it as
    // if it were. The sort is stable, so ties keep suite order, and with no
    // estimates at all the schedule is suite order.
    units.sort_by_key(|u| std::cmp::Reverse(cost(estimates, &u.target).unwrap_or(u64::MAX)));
    units
}

/// What one iteration of each phase of a benchmark costs, keyed by
/// `Target::wasm`.
type Estimates = BTreeMap<PathBuf, BTreeMap<Phase, u64>>;

/// What one iteration of `target` costs. Measuring one phase still runs the
/// phases before it, so it costs them too.
fn cost(estimates: &Estimates, target: &Target) -> Option<u64> {
    let phases = estimates.get(&target.wasm)?;
    Some(match target.phase {
        Some(phase) => phases.range(..=phase).map(|(_, c)| c).sum(),
        None => phases.values().sum(),
    })
}

/// The event a unit's cost is measured in.
const COST_EVENT: &str = "virtual-cycles";

/// How many recorded runs to look back through for an estimate. A benchmark
/// missing from all of them is new or persistently failing, and reading every
/// run to learn that would grow without bound.
const LOOKBACK: usize = 3;

/// Estimate each of `wasms`' cost from the most recent run that measured it.
///
/// Reads visible runs newest first, stopping once every benchmark has an
/// estimate, so the usual case reads one. Benchmarks no recent run measured
/// are absent. An unreadable run is skipped rather than failing the run this
/// only schedules.
fn estimates(data: &DataDir, wasms: &[PathBuf]) -> Result<Estimates> {
    let wanted: BTreeSet<&Path> = wasms.iter().map(PathBuf::as_path).collect();
    let mut estimates = Estimates::new();
    for run in data.visible_runs()?.into_iter().rev().take(LOOKBACK) {
        if estimates.len() == wanted.len() {
            break;
        }
        let measurements = match data.measurements(run) {
            Ok(measurements) => measurements,
            Err(e) => {
                log::warn!("not estimating from run {run}: {e:#}");
                continue;
            }
        };
        for (wasm, cost) in costs(&measurements) {
            if wanted.contains(wasm.as_path()) {
                // The first run to measure it is the newest one that did.
                estimates.entry(wasm).or_insert(cost);
            }
        }
    }
    Ok(estimates)
}

/// What one iteration of each phase of each benchmark in `measurements`
/// costs: its median `COST_EVENT` count.
fn costs(measurements: &[Measurement<'_>]) -> Estimates {
    let mut samples: BTreeMap<(&str, Phase), Vec<u64>> = BTreeMap::new();
    for m in measurements.iter().filter(|m| m.event == COST_EVENT) {
        samples.entry((&m.wasm, m.phase)).or_default().push(m.count);
    }
    let mut costs = Estimates::new();
    for ((wasm, phase), mut counts) in samples {
        counts.sort_unstable();
        costs
            .entry(PathBuf::from(wasm))
            .or_default()
            .insert(phase, counts[counts.len() / 2]);
    }
    costs
}

/// Parse a Sightglass `.suite` file the way `sightglass-cli` does (its parser
/// is private to the CLI crate): one path per line, relative to the suite
/// file, `#` comments and blank lines ignored. Paths under `benchmarks_dir`
/// are returned relative to it.
pub fn parse_suite(suite: &Path, benchmarks_dir: &Path) -> Result<Vec<PathBuf>> {
    let text = std::fs::read_to_string(suite)
        .with_context(|| format!("reading suite {}", suite.display()))?;
    let parent = suite.parent().unwrap_or(Path::new("."));
    let benchmarks_dir = normalize(benchmarks_dir);
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let path = normalize(&parent.join(line));
            match path.strip_prefix(&benchmarks_dir) {
                Ok(relative) => relative.to_path_buf(),
                Err(_) => path,
            }
        })
        .collect())
}

/// Resolve `.` and `..` without touching the file system, which would also
/// resolve symlinks.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            c => out.push(c),
        }
    }
    out
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}h{:02}m{:02}s", s / 3600, s / 60 % 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn config() -> Config {
        toml::from_str(
            r#"
            [benchmark]
            suite = "benchmarks/test.suite"
            iterations = 3
            engine-flags = "--target x86_64-unknown-linux-gnu -Ccranelift-has-avx"
            [regressions]
            significance-level = 0.01
            issue-repo = "a/b"
            issue-labels = ""
            [regressions.min-relative-change.compilation]
            geomean = 0.002
            individual = 0.01
            [regressions.min-relative-change.instantiation]
            geomean = 0.02
            individual = 0.05
            [regressions.min-relative-change.execution]
            geomean = 0.002
            individual = 0.01
            [bisect]
            iterations = 2
            blame-share = 0.25
            completeness-tolerance = 0.25
            [site]
            base-url = "https://example.com/"
            repo = "a/b"
            "#,
        )
        .unwrap()
    }

    /// A repo root with a fake `sightglass/benchmarks/` holding two suites.
    fn root() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let benchmarks = tmp.path().join("sightglass/benchmarks");
        std::fs::create_dir_all(&benchmarks).unwrap();
        std::fs::write(
            benchmarks.join("test.suite"),
            "# A comment.\n\na/benchmark.wasm\n  b/b.wasm  \nc/benchmark.wasm\n",
        )
        .unwrap();
        std::fs::write(benchmarks.join("one.suite"), "b/b.wasm\n").unwrap();
        tmp
    }

    fn options(root: &Path) -> Options {
        let engine = root.join("libengine.so");
        std::fs::write(&engine, "").unwrap();
        Options {
            engine,
            sightglass_cli: "sightglass-cli".into(),
            out: root.join("out"),
            suite: None,
            pairs: vec![],
            iterations: None,
            jobs: Some(2),
            shard: Shard::ALL,
        }
    }

    fn host() -> Host {
        Host {
            uname: "Linux fake".into(),
            cpu: "Fake CPU".into(),
            cpu_flags: "fpu avx2".into(),
        }
    }

    fn run_fake(options: &Options, runner: &dyn Runner) -> Result<Manifest> {
        let root = options.out.parent().unwrap();
        run_with(root, &config(), options, runner, host())
    }

    /// Writes a one-row CSV, or fails for wasm paths containing `fail`, and
    /// records the maximum number of concurrent calls.
    #[derive(Default)]
    struct FakeRunner {
        calls: Mutex<Vec<Vec<OsString>>>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
        fail: Vec<(&'static str, u32)>,
    }

    impl Runner for FakeRunner {
        fn run(&self, _: &Path, args: &[OsString], cwd: &Path) -> Result<Exit> {
            assert!(cwd.ends_with("sightglass/benchmarks"));
            let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_in_flight.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(10));
            self.calls.lock().unwrap().push(args.to_vec());
            self.in_flight.fetch_sub(1, Ordering::SeqCst);

            let arg = |flag: &str| {
                let i = args.iter().position(|a| a == flag).unwrap();
                args[i + 1].to_str().unwrap().to_string()
            };
            let wasm = args.last().unwrap().to_str().unwrap();
            let csv = PathBuf::from(arg("--output-file"));
            let iteration: u32 = csv
                .to_str()
                .unwrap()
                .rsplit('.')
                .nth(1)
                .unwrap()
                .parse()
                .unwrap();
            if self
                .fail
                .iter()
                .any(|&(w, i)| wasm.contains(w) && i == iteration)
            {
                return Ok(Exit {
                    success: false,
                    status: "exit status: 1".into(),
                    stderr: "some output\nboom\n\n".into(),
                });
            }
            std::fs::write(
                csv,
                format!(
                    "arch,engine,engine_flags,wasm,phase,event,count\n\
                     x86_64,wasmtime,,{wasm},Execution,virtual-cycles,100\n"
                ),
            )
            .unwrap();
            Ok(Exit {
                success: true,
                status: "exit status: 0".into(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn command_lines() {
        let unit = Unit {
            target: Target {
                wasm: "a/benchmark.wasm".into(),
                phase: None,
            },
            iteration: 2,
        };
        assert_eq!(unit.csv_name(), "a__benchmark.wasm.002.csv");
        let args = command_line(
            &config(),
            Path::new("/e/libengine.so"),
            Path::new("/out/a__benchmark.wasm.002.csv"),
            &unit,
        );
        let expected: Vec<OsString> = [
            "benchmark",
            "--measure",
            "callgrind",
            "--iters",
            "1",
            "--raw",
            "--output-format",
            "csv",
            "--output-file",
            "/out/a__benchmark.wasm.002.csv",
            "--engine",
            "/e/libengine.so",
            "--name",
            "wasmtime",
            "--engine-flags-common=--target x86_64-unknown-linux-gnu -Ccranelift-has-avx",
            "--",
            "a/benchmark.wasm",
        ]
        .map(OsString::from)
        .to_vec();
        assert_eq!(args, expected);

        let unit = Unit {
            target: Target {
                wasm: "a/benchmark.wasm".into(),
                phase: Some(Phase::Execution),
            },
            iteration: 2,
        };
        assert_eq!(unit.csv_name(), "a__benchmark.wasm.execution.002.csv");
        let args = command_line(
            &config(),
            Path::new("/e/libengine.so"),
            Path::new("/out/x.csv"),
            &unit,
        );
        let tail: Vec<&str> = args[args.len() - 4..]
            .iter()
            .map(|a| a.to_str().unwrap())
            .collect();
        assert_eq!(
            tail,
            ["--benchmark-phase", "execution", "--", "a/benchmark.wasm"]
        );
    }

    #[test]
    fn runs_only_the_given_pairs() {
        let root = root();
        let runner = FakeRunner {
            fail: vec![("b/", 1)],
            ..Default::default()
        };
        let options = Options {
            pairs: vec![
                "compilation:b/b.wasm".parse().unwrap(),
                "execution:b/b.wasm".parse().unwrap(),
            ],
            iterations: Some(2),
            ..options(root.path())
        };
        let mut config = config();
        config.benchmark.iterations = 2;
        let manifest = run_with(root.path(), &config, &options, &runner, host()).unwrap();
        let mut files: Vec<String> = std::fs::read_dir(&options.out)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "b__b.wasm.compilation.000.csv",
                "b__b.wasm.execution.000.csv",
                MANIFEST
            ]
        );
        let failed: Vec<(u32, Option<Phase>)> = manifest
            .failed
            .iter()
            .map(|f| (f.iteration, f.phase))
            .collect();
        assert_eq!(
            failed,
            [(1, Some(Phase::Compilation)), (1, Some(Phase::Execution))]
        );
    }

    #[test]
    fn pair_syntax() {
        let pair: Pair = "Execution:bz2/benchmark.wasm".parse().unwrap();
        assert_eq!(pair.phase, Phase::Execution);
        assert_eq!(pair.wasm, "bz2/benchmark.wasm");
        assert_eq!(pair.to_string(), "execution:bz2/benchmark.wasm");
        for bad in ["bz2/benchmark.wasm", "running:bz2.wasm", "execution:"] {
            assert!(bad.parse::<Pair>().is_err(), "{bad}");
        }
    }

    #[test]
    fn runs_every_unit() {
        let root = root();
        let runner = FakeRunner::default();
        let manifest = run_fake(&options(root.path()), &runner).unwrap();
        assert!(manifest.failed.is_empty());

        let mut wasms: Vec<String> = runner
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|a| a.last().unwrap().to_str().unwrap().to_string())
            .collect();
        wasms.sort();
        assert_eq!(
            wasms,
            [
                "a/benchmark.wasm",
                "a/benchmark.wasm",
                "a/benchmark.wasm",
                "b/b.wasm",
                "b/b.wasm",
                "b/b.wasm",
                "c/benchmark.wasm",
                "c/benchmark.wasm",
                "c/benchmark.wasm"
            ]
        );
        let out = root.path().join("out");
        assert!(out.join("b__b.wasm.000.csv").exists());
        assert!(out.join("c__benchmark.wasm.002.csv").exists());
        let written: Manifest =
            toml::from_str(&std::fs::read_to_string(out.join(MANIFEST)).unwrap()).unwrap();
        assert_eq!(written, manifest);
        assert_eq!((written.index, written.count), (0, 1));
        assert_eq!(written.host, host());
    }

    #[test]
    fn runs_only_its_shard() {
        let root = root();
        let runner = FakeRunner::default();
        let options = Options {
            suite: Some("benchmarks/one.suite".into()),
            iterations: None,
            shard: "1/2".parse().unwrap(),
            ..options(root.path())
        };
        // The fixture config runs 3 iterations, so shard 1 of 2 runs just 1.
        let manifest = run_fake(&options, &runner).unwrap();
        assert_eq!((manifest.index, manifest.count), (1, 2));
        let mut files: Vec<String> = std::fs::read_dir(&options.out)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(files, ["b__b.wasm.001.csv", MANIFEST]);
    }

    #[test]
    fn empty_shard_is_an_error() {
        let root = root();
        let options = Options {
            shard: "3/4".parse().unwrap(),
            ..options(root.path())
        };
        let err = run_fake(&options, &FakeRunner::default()).unwrap_err();
        assert!(
            err.to_string().contains("shard 3/4 has no iterations"),
            "{err}"
        );
    }

    #[test]
    fn shard_syntax() {
        let shard: Shard = "2/3".parse().unwrap();
        assert_eq!(shard, Shard { index: 2, count: 3 });
        assert_eq!(shard.to_string(), "2/3");
        assert!(shard.runs(2) && shard.runs(5) && !shard.runs(3));
        for bad in ["3/3", "1", "a/3", "1/0", "-1/3"] {
            assert!(bad.parse::<Shard>().is_err(), "{bad}");
        }
    }

    #[test]
    fn cpuinfo_fields() {
        let info = "processor\t: 0\nmodel name\t: AMD EPYC 7763\nflags\t\t: fpu vme\n\nprocessor\t: 1\nmodel name\t: AMD EPYC 7763\n";
        assert_eq!(cpuinfo_field(info, "model name").unwrap(), "AMD EPYC 7763");
        assert_eq!(cpuinfo_field(info, "flags").unwrap(), "fpu vme");
        assert_eq!(cpuinfo_field(info, "bogus"), None);
    }

    #[test]
    fn suite_override() {
        let root = root();
        let runner = FakeRunner::default();
        let options = Options {
            suite: Some("benchmarks/one.suite".into()),
            ..options(root.path())
        };
        run_fake(&options, &runner).unwrap();
        let calls = runner.calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert!(calls.iter().all(|a| a.last().unwrap() == "b/b.wasm"));
    }

    #[test]
    fn respects_jobs() {
        let root = root();
        for jobs in [1, 4] {
            let runner = FakeRunner::default();
            let options = Options {
                jobs: Some(jobs),
                out: root.path().join(format!("out{jobs}")),
                ..options(root.path())
            };
            run_fake(&options, &runner).unwrap();
            let max = runner.max_in_flight.load(Ordering::SeqCst);
            assert!(max <= jobs, "{max} > {jobs}");
            if jobs > 1 {
                assert!(max > 1, "never ran in parallel");
            }
        }
    }

    #[test]
    fn collects_failures() {
        let root = root();
        let runner = FakeRunner {
            fail: vec![("a/", 0), ("a/", 2), ("c/", 1)],
            ..Default::default()
        };
        let manifest = run_fake(&options(root.path()), &runner).unwrap();
        // Every unit still ran.
        assert_eq!(runner.calls.lock().unwrap().len(), 9);
        let failed = |wasm: &str, iteration| Failed {
            wasm: wasm.into(),
            iteration,
            phase: None,
            reason: "exit status: 1: boom".into(),
        };
        let expected = [
            failed("a/benchmark.wasm", 0),
            failed("a/benchmark.wasm", 2),
            failed("c/benchmark.wasm", 1),
        ];
        assert_eq!(manifest.failed, expected);
        let written: Manifest = toml::from_str(
            &std::fs::read_to_string(root.path().join("out").join(MANIFEST)).unwrap(),
        )
        .unwrap();
        assert_eq!(written.failed, expected);
    }

    #[test]
    fn missing_output_is_a_failure() {
        struct NoOutput;
        impl Runner for NoOutput {
            fn run(&self, _: &Path, args: &[OsString], _: &Path) -> Result<Exit> {
                let wasm = args.last().unwrap().to_str().unwrap();
                if wasm.starts_with("a/") {
                    return FakeRunner::default().run(
                        Path::new(""),
                        args,
                        Path::new("sightglass/benchmarks"),
                    );
                }
                Ok(Exit {
                    success: true,
                    status: "exit status: 0".into(),
                    stderr: String::new(),
                })
            }
        }
        let root = root();
        let manifest = run_fake(&options(root.path()), &NoOutput).unwrap();
        // `b` and `c`, three iterations each.
        assert_eq!(manifest.failed.len(), 6);
        assert!(
            manifest
                .failed
                .iter()
                .filter(|f| f.wasm == "b/b.wasm")
                .all(|f| f.reason.contains("no output file"))
        );
    }

    #[test]
    fn all_failed_is_an_error() {
        let root = root();
        // `one.suite` is just `b/b.wasm`.
        let runner = FakeRunner {
            fail: vec![("b/", 0), ("b/", 1), ("b/", 2)],
            ..Default::default()
        };
        let options = Options {
            suite: Some("benchmarks/one.suite".into()),
            ..options(root.path())
        };
        let err = run_fake(&options, &runner).unwrap_err();
        assert!(err.to_string().contains("all 3 units failed"), "{err}");
    }

    #[test]
    fn refuses_non_empty_out() {
        let root = root();
        let options = options(root.path());
        std::fs::create_dir(&options.out).unwrap();
        std::fs::write(options.out.join("stale.csv"), "").unwrap();
        assert!(run_fake(&options, &FakeRunner::default()).is_err());
    }

    /// A `benchmark-data/` holding one directory per run, each measuring the
    /// given `(wasm, phase, count)` rows six times.
    fn data_dir(runs: &[&[(&str, Phase, u64)]]) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let data = DataDir::new(tmp.path());
        for (i, rows) in runs.iter().enumerate() {
            let dir = data.run_dir(crate::runs::RunNumber::new(i as u32 + 1).unwrap());
            std::fs::create_dir_all(&dir).unwrap();
            let measurements: Vec<Measurement> = rows
                .iter()
                .flat_map(|&(wasm, phase, count)| {
                    // Spread the samples around `count` so the median is it.
                    [count - 2, count - 1, count, count, count + 1, count + 2].map(move |count| {
                        Measurement {
                            arch: "x86_64".into(),
                            engine: "wasmtime".into(),
                            wasm: wasm.into(),
                            phase,
                            event: COST_EVENT.into(),
                            count,
                        }
                    })
                })
                .collect();
            let file = std::fs::File::create(dir.join(crate::runs::CSV)).unwrap();
            crate::runs::write_csv_gz(&measurements, file).unwrap();
        }
        tmp
    }

    /// One iteration of every phase of `wasm`.
    fn total(estimates: &Estimates, wasm: &str) -> u64 {
        let target = Target {
            wasm: wasm.into(),
            phase: None,
        };
        cost(estimates, &target).unwrap()
    }

    fn targets(wasms: &[&str]) -> Vec<Target> {
        wasms
            .iter()
            .map(|w| Target {
                wasm: w.into(),
                phase: None,
            })
            .collect()
    }

    #[test]
    fn costs_sum_phase_medians() {
        let measurements: Vec<Measurement> = [
            ("a.wasm", Phase::Compilation, 10),
            ("a.wasm", Phase::Compilation, 30),
            ("a.wasm", Phase::Execution, 100),
            ("a.wasm", Phase::Execution, 300),
            ("b.wasm", Phase::Execution, 7),
        ]
        .into_iter()
        .map(|(wasm, phase, count)| Measurement {
            arch: "x86_64".into(),
            engine: "wasmtime".into(),
            wasm: wasm.into(),
            phase,
            event: COST_EVENT.into(),
            count,
        })
        // An event we don't cost by must not count toward the estimate.
        .chain([Measurement {
            arch: "x86_64".into(),
            engine: "wasmtime".into(),
            wasm: "b.wasm".into(),
            phase: Phase::Execution,
            event: "instructions-retired".into(),
            count: 10_000,
        }])
        .collect();
        let costs = costs(&measurements);
        let target = |wasm: &str, phase| Target {
            wasm: wasm.into(),
            phase,
        };
        assert_eq!(cost(&costs, &target("a.wasm", None)), Some(330));
        assert_eq!(cost(&costs, &target("b.wasm", None)), Some(7));
        // Measuring a later phase still runs the earlier ones.
        assert_eq!(
            cost(&costs, &target("a.wasm", Some(Phase::Compilation))),
            Some(30)
        );
        assert_eq!(
            cost(&costs, &target("a.wasm", Some(Phase::Execution))),
            Some(330)
        );
        assert_eq!(cost(&costs, &target("c.wasm", None)), None);
    }

    #[test]
    fn estimates_come_from_the_newest_run_measuring_each() {
        let tmp = data_dir(&[
            &[("a/benchmark.wasm", Phase::Execution, 500)],
            &[
                ("a/benchmark.wasm", Phase::Execution, 100),
                ("b/b.wasm", Phase::Execution, 200),
            ],
        ]);
        let wasms = [PathBuf::from("a/benchmark.wasm"), PathBuf::from("b/b.wasm")];
        let estimates = estimates(&DataDir::new(tmp.path()), &wasms).unwrap();
        assert_eq!(total(&estimates, "a/benchmark.wasm"), 100);
        assert_eq!(total(&estimates, "b/b.wasm"), 200);
    }

    #[test]
    fn estimates_fall_back_to_an_older_run() {
        // `b` is in the oldest run only, and `c` in none within the lookback.
        let mut runs: Vec<&[(&str, Phase, u64)]> = vec![&[
            ("a/benchmark.wasm", Phase::Execution, 9),
            ("b/b.wasm", Phase::Execution, 7),
        ]];
        let later: &[(&str, Phase, u64)] = &[("a/benchmark.wasm", Phase::Execution, 5)];
        runs.extend(std::iter::repeat_n(later, LOOKBACK));
        let tmp = data_dir(&runs);
        let wasms = [
            PathBuf::from("a/benchmark.wasm"),
            PathBuf::from("b/b.wasm"),
            PathBuf::from("c/c.wasm"),
        ];
        let estimates = estimates(&DataDir::new(tmp.path()), &wasms).unwrap();
        assert_eq!(total(&estimates, "a/benchmark.wasm"), 5);
        // Beyond the lookback.
        assert!(!estimates.contains_key(Path::new("b/b.wasm")));
        assert!(!estimates.contains_key(Path::new("c/c.wasm")));
    }

    #[test]
    fn estimates_skip_excluded_runs() {
        let tmp = data_dir(&[
            &[("a/benchmark.wasm", Phase::Execution, 100)],
            &[("a/benchmark.wasm", Phase::Execution, 999)],
        ]);
        std::fs::write(
            DataDir::new(tmp.path()).path().join(crate::runs::EXCLUDED),
            "[[excluded]]\nrun = 2\nreason = \"bad\"\n",
        )
        .unwrap();
        let wasms = [PathBuf::from("a/benchmark.wasm")];
        let estimates = estimates(&DataDir::new(tmp.path()), &wasms).unwrap();
        assert_eq!(total(&estimates, "a/benchmark.wasm"), 100);
    }

    #[test]
    fn estimates_with_no_data() {
        let tmp = tempfile::tempdir().unwrap();
        let wasms = [PathBuf::from("a/benchmark.wasm")];
        assert!(
            estimates(&DataDir::new(tmp.path()), &wasms)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn unreadable_runs_are_skipped() {
        let tmp = data_dir(&[&[("a/benchmark.wasm", Phase::Execution, 100)]]);
        let data = DataDir::new(tmp.path());
        let dir = data.run_dir(crate::runs::RunNumber::new(2).unwrap());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::runs::CSV), "not gzipped").unwrap();
        let wasms = [PathBuf::from("a/benchmark.wasm")];
        let estimates = estimates(&data, &wasms).unwrap();
        assert_eq!(total(&estimates, "a/benchmark.wasm"), 100);
    }

    #[test]
    fn schedules_longest_first() {
        let targets = targets(&["a.wasm", "b.wasm", "c.wasm", "d.wasm"]);
        let estimates: Estimates = [("a.wasm", 10), ("b.wasm", 30), ("d.wasm", 20)]
            .into_iter()
            .map(|(w, c)| (PathBuf::from(w), [(Phase::Execution, c)].into()))
            .collect();
        let order: Vec<String> = schedule(&targets, 2, Shard::ALL, &estimates)
            .iter()
            .map(|u| format!("{} {}", u.target.wasm.display(), u.iteration))
            .collect();
        assert_eq!(
            order,
            [
                // Unestimated, so assumed longest.
                "c.wasm 0", "c.wasm 1", "b.wasm 0", "b.wasm 1", "d.wasm 0", "d.wasm 1", "a.wasm 0",
                "a.wasm 1"
            ]
        );
    }

    #[test]
    fn schedules_in_suite_order_without_estimates() {
        let targets = targets(&["b.wasm", "a.wasm", "c.wasm"]);
        let units = schedule(&targets, 1, Shard::ALL, &Estimates::new());
        let order: Vec<&Path> = units.iter().map(|u| u.target.wasm.as_path()).collect();
        assert_eq!(
            order,
            [
                Path::new("b.wasm"),
                Path::new("a.wasm"),
                Path::new("c.wasm")
            ]
        );
    }

    #[test]
    fn suite_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let benchmarks = tmp.path().join("sightglass/benchmarks");
        std::fs::create_dir_all(&benchmarks).unwrap();
        let elsewhere = tmp.path().join("elsewhere.suite");
        std::fs::write(
            &elsewhere,
            "sightglass/benchmarks/x/benchmark.wasm\n./sightglass/benchmarks/../benchmarks/y.wasm\nz.wasm\n",
        )
        .unwrap();
        assert_eq!(
            parse_suite(&elsewhere, &benchmarks).unwrap(),
            [
                PathBuf::from("x/benchmark.wasm"),
                PathBuf::from("y.wasm"),
                tmp.path().join("z.wasm"),
            ]
        );
    }

    #[test]
    fn real_suite() {
        let benchmarks = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sightglass/benchmarks");
        let wasms = parse_suite(&benchmarks.join("default.suite"), &benchmarks).unwrap();
        assert!(wasms.len() > 10, "{wasms:?}");
        for wasm in &wasms {
            assert!(wasm.is_relative(), "{}", wasm.display());
            assert!(benchmarks.join(wasm).exists(), "{}", wasm.display());
        }
        assert!(wasms.contains(&PathBuf::from("richards/benchmark.wasm")));
    }
}
