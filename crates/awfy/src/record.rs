//! `awfy record`: turn the scratch directories of every shard's `awfy run`
//! into a new, immutable `benchmark-data/NNNNN/`. See `docs/data.md`.

use crate::config::Config;
use crate::git::{GitLog, Repo};
use crate::metadata::{
    AreWeFastYet, Environment, Host, Metadata, RunConfig, SCHEMA, Sightglass, Timestamp, Wasmtime,
};
use crate::run::{Failed, MANIFEST, Manifest};
use crate::runs::{self, DataDir, RunNumber};
use anyhow::{Context, Result, bail};
use sightglass_data::Measurement;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Options for the `awfy record` command.
#[derive(clap::Args)]
pub struct Options {
    /// `awfy run`'s output directory. Repeat once per shard; together they
    /// must cover every shard of the run exactly once.
    #[arg(long, required = true)]
    pub scratch: Vec<PathBuf>,
    /// Full SHA of the benchmarked Wasmtime commit.
    #[arg(long)]
    pub wasmtime_commit: String,
    /// Its committer date, RFC 3339.
    #[arg(long)]
    pub wasmtime_commit_date: Timestamp,
    /// When the run started, RFC 3339.
    #[arg(long)]
    pub started: Timestamp,
    /// URL of the workflow run.
    #[arg(long)]
    pub workflow_run: Option<String>,
    /// The suite passed to `awfy run --suite`, if any.
    #[arg(long)]
    pub suite: Option<String>,
    /// The iterations passed to `awfy run --iterations`, if any.
    #[arg(long)]
    pub iterations: Option<u32>,
    /// A Wasmtime clone, for the commits new since the previous run.
    #[arg(long)]
    pub wasmtime_repo: Option<PathBuf>,
}

/// Most commits recorded in `wasmtime.commits`.
///
/// Runs are three hours apart, so this only bites after a long gap. The rest
/// are counted in `wasmtime.omitted-commits` and stay reachable through the
/// GitHub compare link.
pub const MAX_COMMITS: usize = 200;

/// Everything `record` learns from outside the scratch directory, so it can be
/// faked in tests.
pub trait SystemMethods {
    /// Trimmed stdout of a command that must succeed.
    fn command(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<String>;
    /// Get the timestamp for the current moment.
    fn now(&self) -> Timestamp;
}

/// The real system.
struct System;

impl SystemMethods for System {
    fn command(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<String> {
        let output = std::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .with_context(|| format!("running `{program}`"))?;
        anyhow::ensure!(
            output.status.success(),
            "`{program} {}` failed: {}\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_string())
    }

    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// Run `awfy record` with the given options.
pub fn run(root: &Path, options: Options) -> Result<()> {
    let repo = options.wasmtime_repo.as_deref().map(Repo::new);
    let run = record(
        root,
        &options,
        &System,
        repo.as_ref().map(|r| r as &dyn GitLog),
    )?;
    println!("{run}");
    Ok(())
}

/// Write the new run directory and return its number.
pub fn record(
    root: &Path,
    options: &Options,
    probe: &dyn SystemMethods,
    git: Option<&dyn GitLog>,
) -> Result<RunNumber> {
    let config = Config::load(root)?;
    let manifests = manifests(&options.scratch)?;
    let measurements = merge(&options.scratch)?;
    if measurements.is_empty() {
        bail!(
            "no benchmark produced any rows in {}",
            scratch_list(&options.scratch)
        );
    }
    let failures = merge_failures(&manifests);
    let iterations = options.iterations.unwrap_or(config.benchmark.iterations);

    let data = DataDir::new(root);
    std::fs::create_dir_all(data.path())?;
    let run = data.next_run_number()?;
    // The log is recorded now because the site is built elsewhere, by a job
    // with no Wasmtime clone to read it from.
    let previous = data.visible_runs()?.last().copied();
    let (commits, omitted_commits) = match (git, previous) {
        (Some(git), Some(previous)) => {
            let previous = data.metadata(previous)?.wasmtime.commit;
            let mut commits = git.log(&previous, &options.wasmtime_commit)?;
            let omitted = commits.len().saturating_sub(MAX_COMMITS);
            commits.truncate(MAX_COMMITS);
            (commits, u32::try_from(omitted).unwrap_or(u32::MAX))
        }
        _ => (vec![], 0),
    };
    let target = data.run_dir(run);
    if target.exists() {
        bail!("{} already exists", target.display());
    }

    // Build the run in a temp dir, then rename it into place, so a failure
    // never leaves a partial run. `runs()` ignores the temp dir's name.
    let tmp = tempfile::Builder::new()
        .prefix(".record-")
        .tempdir_in(data.path())?;
    let csv = std::fs::File::create(tmp.path().join(runs::CSV))?;
    runs::write_csv_gz(&measurements, std::io::BufWriter::new(csv))?;

    let metadata = Metadata {
        schema: SCHEMA,
        started: options.started,
        finished: probe.now(),
        wasmtime: Wasmtime {
            commit: options.wasmtime_commit.clone(),
            commit_date: options.wasmtime_commit_date,
            omitted_commits,
            commits,
        },
        sightglass: Sightglass {
            commit: probe.command(&root.join("sightglass"), "git", &["rev-parse", "HEAD"])?,
            suite: options
                .suite
                .clone()
                .unwrap_or_else(|| config.benchmark.suite.clone()),
        },
        are_we_fast_yet: AreWeFastYet {
            commit: probe.command(root, "git", &["rev-parse", "HEAD"])?,
            workflow_run: options.workflow_run.clone(),
        },
        environment: Environment {
            valgrind: probe.command(root, "valgrind", &["--version"])?,
            // Run from the root so `rust-toolchain.toml` picks the toolchain,
            // the same one that built Wasmtime.
            rustc: probe.command(root, "rustc", &["--version"])?,
            hosts: manifests
                .iter()
                .map(|m| Host {
                    shard: m.index,
                    iterations: (0..iterations).filter(|i| i % m.count == m.index).collect(),
                    uname: m.host.uname.clone(),
                    cpu: m.host.cpu.clone(),
                    cpu_flags: Some(m.host.cpu_flags.clone()),
                })
                .collect(),
        },
        config: RunConfig {
            iterations,
            engine_flags: config.benchmark.engine_flags.clone(),
        },
        failures,
    };
    std::fs::write(tmp.path().join(runs::METADATA), metadata.to_toml()?)?;

    std::fs::rename(tmp.keep(), &target)
        .with_context(|| format!("renaming into {}", target.display()))?;
    log::info!(
        "recorded run {run}: {} rows, {} failed benchmark(s), {} new commit(s)",
        measurements.len(),
        metadata.failures.len(),
        metadata.wasmtime.commits.len() + metadata.wasmtime.omitted_commits as usize
    );
    Ok(run)
}

fn scratch_list(scratch: &[PathBuf]) -> String {
    let dirs: Vec<String> = scratch.iter().map(|d| d.display().to_string()).collect();
    dirs.join(", ")
}

/// Every scratch directory's manifest, in shard order. Errors unless they
/// cover every shard of one run exactly once, so a lost shard can never pass
/// for a run with fewer iterations.
fn manifests(scratch: &[PathBuf]) -> Result<Vec<Manifest>> {
    let mut manifests = Vec::new();
    for dir in scratch {
        let path = dir.join(MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        manifests.push(manifest);
    }
    manifests.sort_by_key(|m| m.index);
    let count = manifests.first().context("no scratch directories")?.count;
    let indices: Vec<u32> = manifests.iter().map(|m| m.index).collect();
    if manifests.iter().any(|m| m.count != count) || indices != (0..count).collect::<Vec<_>>() {
        let shards: Vec<String> = manifests
            .iter()
            .map(|m| format!("{}/{}", m.index, m.count))
            .collect();
        bail!(
            "the scratch directories must hold each shard of one run exactly once; found shards {}",
            shards.join(", ")
        );
    }
    Ok(manifests)
}

/// One entry per benchmark, e.g. `"iteration 0: exit status: 1: boom;
/// iteration 2: ..."`.
fn merge_failures(manifests: &[Manifest]) -> BTreeMap<String, String> {
    let mut failed: Vec<&Failed> = manifests.iter().flat_map(|m| &m.failed).collect();
    failed.sort();
    let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in failed {
        merged
            .entry(f.wasm.clone())
            .or_default()
            .push(format!("iteration {}: {}", f.iteration, f.reason));
    }
    merged
        .into_iter()
        .map(|(wasm, reasons)| (wasm, reasons.join("; ")))
        .collect()
}

/// Read every per-unit CSV in `scratch`, normalize, and sort.
///
/// `engine` becomes `wasmtime`, and `wasm` is made relative to
/// `sightglass/benchmarks/`.
fn merge(scratch: &[PathBuf]) -> Result<Vec<Measurement<'static>>> {
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in scratch {
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "csv") {
                files.push(path);
            }
        }
    }
    // By file name, not path, so each benchmark's rows stay in iteration
    // order whichever shard measured them.
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    if let Some(w) = files
        .windows(2)
        .find(|w| w[0].file_name() == w[1].file_name())
    {
        bail!(
            "{} and {} measured the same unit",
            w[0].display(),
            w[1].display()
        );
    }

    let mut measurements = Vec::new();
    for file in files {
        let rows = runs::read_csv(std::fs::File::open(&file)?)
            .with_context(|| format!("reading {}", file.display()))?;
        for mut m in rows {
            m.engine.name = "wasmtime".into();
            m.wasm = normalize_wasm(&m.wasm)
                .with_context(|| format!("in {}", file.display()))?
                .into();
            measurements.push(m);
        }
    }
    // Stable, and files are read in sorted order, so equal keys keep their
    // file order and the gzipped output stays byte-reproducible.
    measurements.sort_by(|a, b| (&a.wasm, a.phase, &a.event).cmp(&(&b.wasm, b.phase, &b.event)));
    Ok(measurements)
}

/// Make a wasm path relative to `sightglass/benchmarks/`: strip everything up
/// to and including the last `benchmarks` directory. A path without one must
/// already be relative to it.
fn normalize_wasm(wasm: &str) -> Result<String> {
    let path = Path::new(wasm);
    let components: Vec<Component> = path.components().collect();
    let rest = match components
        .iter()
        .rposition(|c| c.as_os_str() == "benchmarks")
    {
        Some(i) => &components[i + 1..],
        None => &components[..],
    };
    let mut out = Vec::new();
    for c in rest {
        match c {
            Component::Normal(s) => out.push(s.to_str().context("non-UTF-8 path")?),
            Component::CurDir => {}
            _ => bail!("wasm path `{wasm}` is not under sightglass/benchmarks/"),
        }
    }
    if out.is_empty() {
        bail!("wasm path `{wasm}` names no benchmark");
    }
    Ok(out.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::Commit;
    use std::cell::Cell;

    /// Canned answers. Fails `valgrind` when `fail_valgrind` is set.
    #[derive(Default)]
    struct FakeProbe {
        fail_valgrind: Cell<bool>,
    }

    impl SystemMethods for FakeProbe {
        fn command(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<String> {
            Ok(match (program, args) {
                ("git", ["rev-parse", "HEAD"]) if cwd.ends_with("sightglass") => "s".repeat(40),
                ("git", ["rev-parse", "HEAD"]) => "a".repeat(40),
                ("valgrind", _) if self.fail_valgrind.get() => bail!("no valgrind"),
                ("valgrind", ["--version"]) => "valgrind-3.22.0".into(),
                ("rustc", ["--version"]) => "rustc 1.99.0 (b940084d7 2026-09-28)".into(),
                _ => bail!("unexpected command {program} {args:?}"),
            })
        }
        fn now(&self) -> Timestamp {
            "2026-10-10T04:00:00Z".parse().unwrap()
        }
    }

    const HEADER: &str = "arch,engine,engine_flags,wasm,phase,event,count\n";

    fn host(cpu: &str) -> crate::run::Host {
        crate::run::Host {
            uname: "Linux fake x86_64 GNU/Linux".into(),
            cpu: cpu.into(),
            cpu_flags: format!("{cpu} flags"),
        }
    }

    fn failed(wasm: &str, iteration: u32) -> Failed {
        Failed {
            wasm: wasm.into(),
            iteration,
            reason: "exit status: 1".into(),
        }
    }

    /// Write the given `(file name, rows)` and a manifest into `dir`.
    fn write_shard(dir: &Path, files: &[(&str, &str)], manifest: &Manifest) {
        std::fs::create_dir_all(dir).unwrap();
        for (name, rows) in files {
            std::fs::write(dir.join(name), format!("{HEADER}{rows}")).unwrap();
        }
        std::fs::write(dir.join(MANIFEST), toml::to_string(manifest).unwrap()).unwrap();
    }

    /// An unsharded scratch dir with the given `(file name, rows)` and
    /// failures.
    fn scratch(files: &[(&str, &str)], failed: Vec<Failed>) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = Manifest {
            index: 0,
            count: 1,
            host: host("Fake CPU"),
            failed,
        };
        write_shard(tmp.path(), files, &manifest);
        tmp
    }

    fn two_units() -> tempfile::TempDir {
        scratch(
            &[
                (
                    "b__benchmark.wasm.001.csv",
                    "x86_64,/tmp/libengine.so,-Cfoo,/home/r/sightglass/benchmarks/b/benchmark.wasm,Execution,virtual-cycles,20\n\
                     x86_64,/tmp/libengine.so,-Cfoo,/home/r/sightglass/benchmarks/b/benchmark.wasm,Compilation,virtual-cycles,10\n",
                ),
                (
                    "a__a.wasm.000.csv",
                    "x86_64,wasmtime,-Cfoo,a/a.wasm,Compilation,virtual-cycles,30\n\
                     x86_64,wasmtime,-Cfoo,a/a.wasm,Compilation,instructions-retired,15\n",
                ),
                (
                    "b__benchmark.wasm.000.csv",
                    "x86_64,wasmtime,-Cfoo,../../benchmarks/b/benchmark.wasm,Compilation,virtual-cycles,11\n",
                ),
            ],
            vec![failed("c/c.wasm", 0)],
        )
    }

    fn options(scratch: &Path) -> Options {
        Options {
            scratch: vec![scratch.to_path_buf()],
            wasmtime_commit: "0123456789abcdef0123456789abcdef01234567".into(),
            wasmtime_commit_date: "2026-10-09T22:41:05Z".parse().unwrap(),
            started: "2026-10-10T03:17:00Z".parse().unwrap(),
            workflow_run: None,
            suite: None,
            iterations: None,
            wasmtime_repo: None,
        }
    }

    /// `n` commits, newest first, from the baseline to `wasmtime_commit`.
    struct FakeGit(usize);

    impl GitLog for FakeGit {
        fn log(&self, old: &str, new: &str) -> Result<Vec<Commit>> {
            // 00003 is the fixture root's newest run.
            assert_eq!(old, "3".repeat(40));
            assert_eq!(new, "0123456789abcdef0123456789abcdef01234567");
            Ok((0..self.0)
                .rev()
                .map(|i| Commit {
                    commit: format!("{i:040x}"),
                    subject: format!("Commit number {i}"),
                })
                .collect())
        }
    }

    fn fixture_root() -> tempfile::TempDir {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tmp = tempfile::tempdir().unwrap();
        std::fs::copy(fixtures.join("config.toml"), tmp.path().join("config.toml")).unwrap();
        let data = tmp.path().join("benchmark-data");
        std::fs::create_dir(&data).unwrap();
        for run in ["00001", "00003"] {
            std::fs::create_dir(data.join(run)).unwrap();
            std::fs::copy(
                fixtures
                    .join("benchmark-data")
                    .join(run)
                    .join(runs::METADATA),
                data.join(run).join(runs::METADATA),
            )
            .unwrap();
        }
        tmp
    }

    #[test]
    fn wasm_paths() {
        for (input, want) in [
            ("bz2/benchmark.wasm", "bz2/benchmark.wasm"),
            ("./bz2/benchmark.wasm", "bz2/benchmark.wasm"),
            ("benchmarks/bz2/benchmark.wasm", "bz2/benchmark.wasm"),
            (
                "/home/r/sightglass/benchmarks/bz2/benchmark.wasm",
                "bz2/benchmark.wasm",
            ),
            (
                "../../benchmarks/noop/benchmark.wasm",
                "noop/benchmark.wasm",
            ),
            (
                "./benchmarks/shootout/shootout-sieve.wasm",
                "shootout/shootout-sieve.wasm",
            ),
        ] {
            assert_eq!(normalize_wasm(input).unwrap(), want, "{input}");
        }
        for bad in [
            "/elsewhere/x.wasm",
            "../x.wasm",
            "benchmarks/../x.wasm",
            "/a/benchmarks/",
        ] {
            assert!(normalize_wasm(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn merges_and_normalizes() {
        let scratch = two_units();
        let merged = merge(&[scratch.path().to_path_buf()]).unwrap();
        let rows: Vec<String> = merged
            .iter()
            .map(|m| {
                format!(
                    "{} {} {} {} {}",
                    m.engine.name, m.wasm, m.phase, m.event, m.count
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                "wasmtime a/a.wasm compilation instructions-retired 15",
                "wasmtime a/a.wasm compilation virtual-cycles 30",
                // Equal keys keep file order: `.000` before `.001`.
                "wasmtime b/benchmark.wasm compilation virtual-cycles 11",
                "wasmtime b/benchmark.wasm compilation virtual-cycles 10",
                "wasmtime b/benchmark.wasm execution virtual-cycles 20",
            ]
        );
        // Other columns are untouched.
        assert!(
            merged
                .iter()
                .all(|m| m.engine.flags.as_deref() == Some("-Cfoo"))
        );
        assert!(merged.iter().all(|m| m.arch == "x86_64"));
    }

    #[test]
    fn rejects_paths_outside_benchmarks() {
        let scratch = scratch(
            &[(
                "x.000.csv",
                "x86_64,wasmtime,,/elsewhere/x.wasm,Execution,virtual-cycles,1\n",
            )],
            vec![],
        );
        assert!(merge(&[scratch.path().to_path_buf()]).is_err());
    }

    #[test]
    fn records_a_run() {
        let root = fixture_root();
        let scratch = two_units();
        let options = Options {
            workflow_run: Some("https://example.com/run/1".into()),
            suite: Some("benchmarks/one.suite".into()),
            ..options(scratch.path())
        };
        let run = record(root.path(), &options, &FakeProbe::default(), None).unwrap();
        assert_eq!(run, RunNumber::new(4).unwrap());

        let data = DataDir::new(root.path());
        let metadata = data.metadata(run).unwrap();
        assert_eq!(metadata.sightglass.commit, "s".repeat(40));
        assert_eq!(metadata.sightglass.suite, "benchmarks/one.suite");
        assert_eq!(metadata.are_we_fast_yet.commit, "a".repeat(40));
        assert_eq!(
            metadata.are_we_fast_yet.workflow_run.as_deref(),
            Some("https://example.com/run/1")
        );
        assert_eq!(
            metadata.environment.hosts,
            [Host {
                shard: 0,
                iterations: (0..6).collect(),
                uname: "Linux fake x86_64 GNU/Linux".into(),
                cpu: "Fake CPU".into(),
                cpu_flags: Some("Fake CPU flags".into()),
            }]
        );
        assert_eq!(metadata.finished.to_string(), "2026-10-10T04:00:00Z");
        assert_eq!(metadata.config.iterations, 6);
        assert_eq!(
            metadata.failures,
            [(
                "c/c.wasm".to_string(),
                "iteration 0: exit status: 1".to_string()
            )]
            .into()
        );

        let measurements = data.measurements(run).unwrap();
        assert_eq!(measurements.len(), 5);

        // Only the run directory was added.
        let mut entries: Vec<_> = std::fs::read_dir(data.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        assert_eq!(entries, ["00001", "00003", "00004"]);
    }

    #[test]
    fn records_the_new_commits() {
        let root = fixture_root();
        let scratch = two_units();
        let run = record(
            root.path(),
            &options(scratch.path()),
            &FakeProbe::default(),
            Some(&FakeGit(3)),
        )
        .unwrap();
        let wasmtime = DataDir::new(root.path()).metadata(run).unwrap().wasmtime;
        assert_eq!(wasmtime.omitted_commits, 0);
        assert_eq!(
            wasmtime
                .commits
                .iter()
                .map(|c| c.subject.as_str())
                .collect::<Vec<_>>(),
            ["Commit number 2", "Commit number 1", "Commit number 0"]
        );
    }

    #[test]
    fn long_commit_logs_are_truncated() {
        let root = fixture_root();
        let scratch = two_units();
        let run = record(
            root.path(),
            &options(scratch.path()),
            &FakeProbe::default(),
            Some(&FakeGit(MAX_COMMITS + 7)),
        )
        .unwrap();
        let wasmtime = DataDir::new(root.path()).metadata(run).unwrap().wasmtime;
        assert_eq!(wasmtime.commits.len(), MAX_COMMITS);
        assert_eq!(wasmtime.omitted_commits, 7);
        // The newest commits are the ones kept.
        assert_eq!(wasmtime.commits[0].subject, "Commit number 206");
    }

    /// The very first run has no baseline, so there is nothing to log.
    #[test]
    fn no_commits_without_a_previous_run() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/config.toml"),
            tmp.path().join("config.toml"),
        )
        .unwrap();
        let scratch = two_units();
        struct Never;
        impl GitLog for Never {
            fn log(&self, _old: &str, _new: &str) -> Result<Vec<Commit>> {
                panic!("no baseline to log against");
            }
        }
        let run = record(
            tmp.path(),
            &options(scratch.path()),
            &FakeProbe::default(),
            Some(&Never),
        )
        .unwrap();
        let wasmtime = DataDir::new(tmp.path()).metadata(run).unwrap().wasmtime;
        assert_eq!(wasmtime.commits, []);
        assert_eq!(wasmtime.omitted_commits, 0);
    }

    #[test]
    fn suite_defaults_to_config() {
        let root = fixture_root();
        let scratch = two_units();
        let run = record(
            root.path(),
            &options(scratch.path()),
            &FakeProbe::default(),
            None,
        )
        .unwrap();
        let metadata = DataDir::new(root.path()).metadata(run).unwrap();
        assert_eq!(metadata.sightglass.suite, "benchmarks/default.suite");
    }

    #[test]
    fn gzip_is_reproducible() {
        let scratch = two_units();
        let gzip = || {
            let mut out = Vec::new();
            runs::write_csv_gz(&merge(&[scratch.path().to_path_buf()]).unwrap(), &mut out).unwrap();
            out
        };
        let first = gzip();
        assert_eq!(first, gzip());
        // Gzip header: magic, method, flags (no file name), zero mtime.
        assert_eq!(&first[..8], &[0x1f, 0x8b, 8, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn nothing_written_on_failure() {
        let root = fixture_root();
        let scratch = two_units();
        let probe = FakeProbe::default();
        probe.fail_valgrind.set(true);
        assert!(record(root.path(), &options(scratch.path()), &probe, None).is_err());
        let entries: Vec<_> = std::fs::read_dir(root.path().join("benchmark-data"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(entries.len(), 2, "{entries:?}");
    }

    #[test]
    fn no_rows_is_an_error() {
        let root = fixture_root();
        let scratch = scratch(&[], vec![failed("a/a.wasm", 0)]);
        let err = record(
            root.path(),
            &options(scratch.path()),
            &FakeProbe::default(),
            None,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("no benchmark produced any rows"),
            "{err}"
        );
    }

    /// Three shards of a six-iteration run of `a/a.wasm`, each count its
    /// iteration, in directories listed out of shard order.
    fn three_shards(cpus: [&str; 3]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let tmp = tempfile::tempdir().unwrap();
        let mut dirs = Vec::new();
        for index in [2u32, 0, 1] {
            let dir = tmp.path().join(format!("shard-{index}"));
            let files: Vec<(String, String)> = [index, index + 3]
                .iter()
                .map(|&i| {
                    (
                        format!("a__a.wasm.{i:03}.csv"),
                        format!("x86_64,wasmtime,,a/a.wasm,Execution,virtual-cycles,{i}\n"),
                    )
                })
                .collect();
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(n, r)| (n.as_str(), r.as_str()))
                .collect();
            let failed = match index {
                0 => vec![failed("b/b.wasm", 3)],
                2 => vec![failed("b/b.wasm", 2)],
                _ => vec![],
            };
            let manifest = Manifest {
                index,
                count: 3,
                host: host(cpus[index as usize]),
                failed,
            };
            write_shard(&dir, &files, &manifest);
            dirs.push(dir);
        }
        (tmp, dirs)
    }

    #[test]
    fn records_three_shards() {
        let root = fixture_root();
        let (_tmp, dirs) = three_shards(["CPU A", "CPU B", "CPU A"]);
        let options = Options {
            scratch: dirs,
            ..options(Path::new(""))
        };
        let run = record(root.path(), &options, &FakeProbe::default(), None).unwrap();
        let data = DataDir::new(root.path());
        let counts: Vec<u64> = data
            .measurements(run)
            .unwrap()
            .iter()
            .map(|m| m.count)
            .collect();
        // Iteration order, whichever shard measured each.
        assert_eq!(counts, [0, 1, 2, 3, 4, 5]);
        let metadata = data.metadata(run).unwrap();
        let hosts: Vec<(u32, Vec<u32>, &str)> = metadata
            .environment
            .hosts
            .iter()
            .map(|h| (h.shard, h.iterations.clone(), h.cpu.as_str()))
            .collect();
        assert_eq!(
            hosts,
            [
                (0, vec![0, 3], "CPU A"),
                (1, vec![1, 4], "CPU B"),
                (2, vec![2, 5], "CPU A")
            ]
        );
        // Failures from different shards merge in iteration order.
        assert_eq!(
            metadata.failures,
            [(
                "b/b.wasm".to_string(),
                "iteration 2: exit status: 1; iteration 3: exit status: 1".to_string()
            )]
            .into()
        );
    }

    #[test]
    fn rejects_incomplete_shards() {
        let root = fixture_root();
        let (tmp, dirs) = three_shards(["CPU"; 3]);
        let record_dirs = |scratch: Vec<PathBuf>| {
            let options = Options {
                scratch,
                ..options(Path::new(""))
            };
            record(root.path(), &options, &FakeProbe::default(), None)
                .unwrap_err()
                .to_string()
        };
        // Missing shard 2.
        let err = record_dirs(dirs[1..].to_vec());
        assert!(err.contains("found shards 0/3, 1/3"), "{err}");
        // Shard 2 twice, and no shard 1.
        let err = record_dirs(vec![dirs[0].clone(), dirs[0].clone(), dirs[1].clone()]);
        assert!(err.contains("found shards 0/3, 2/3, 2/3"), "{err}");
        // A shard of a different split.
        let other = tmp.path().join("other");
        write_shard(
            &other,
            &[],
            &Manifest {
                index: 1,
                count: 2,
                host: host("CPU"),
                failed: vec![],
            },
        );
        let err = record_dirs(vec![dirs[1].clone(), dirs[2].clone(), other]);
        assert!(err.contains("each shard of one run exactly once"), "{err}");
        assert_eq!(data_entries(root.path()), ["00001", "00003"]);
    }

    fn data_entries(root: &Path) -> Vec<String> {
        let mut entries: Vec<String> = std::fs::read_dir(root.join("benchmark-data"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        entries
    }

    #[test]
    fn refuses_existing_target() {
        let root = fixture_root();
        // A stray file with the next run's name.
        std::fs::write(root.path().join("benchmark-data/00004"), "").unwrap();
        let scratch = two_units();
        assert!(
            record(
                root.path(),
                &options(scratch.path()),
                &FakeProbe::default(),
                None
            )
            .is_err()
        );
    }
}
