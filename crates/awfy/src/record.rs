//! `awfy record`: turn `awfy run`'s scratch directory into a new, immutable
//! `benchmark-data/NNNNN/`. See `docs/data.md`.

use crate::config::Config;
use crate::git::{GitLog, Repo};
use crate::metadata::{
    AreWeFastYet, Environment, Metadata, RunConfig, SCHEMA, Sightglass, Timestamp, Wasmtime,
};
use crate::run::FAILURES;
use crate::runs::{self, DataDir, RunNumber};
use anyhow::{Context, Result, bail};
use sightglass_data::Measurement;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Options for the `awfy record` command.
#[derive(clap::Args)]
pub struct Options {
    /// `awfy run`'s output directory.
    #[arg(long)]
    pub scratch: PathBuf,
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
    /// The CPU model name.
    fn cpu(&self) -> String;
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

    fn cpu(&self) -> String {
        std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|info| {
                info.lines().find_map(|l| {
                    Some(
                        l.strip_prefix("model name")?
                            .split_once(':')?
                            .1
                            .trim()
                            .to_string(),
                    )
                })
            })
            .unwrap_or_else(|| "unknown".into())
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
    let measurements = merge(&options.scratch)?;
    if measurements.is_empty() {
        bail!(
            "no benchmark produced any rows in {}",
            options.scratch.display()
        );
    }
    let failures_path = options.scratch.join(FAILURES);
    let failures: BTreeMap<String, String> = toml::from_str(
        &std::fs::read_to_string(&failures_path)
            .with_context(|| format!("reading {}", failures_path.display()))?,
    )
    .with_context(|| format!("parsing {}", failures_path.display()))?;

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
            uname: probe.command(root, "uname", &["-a"])?,
            cpu: probe.cpu(),
        },
        config: RunConfig {
            iterations: options.iterations.unwrap_or(config.benchmark.iterations),
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

/// Read every per-unit CSV in `scratch`, normalize, and sort.
///
/// `engine` becomes `wasmtime`, and `wasm` is made relative to
/// `sightglass/benchmarks/`.
fn merge(scratch: &Path) -> Result<Vec<Measurement<'static>>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(scratch)
        .with_context(|| format!("reading {}", scratch.display()))?
        .map(|e| Ok(e?.path()))
        .collect::<Result<_>>()?;
    files.retain(|f| f.extension().is_some_and(|e| e == "csv"));
    files.sort();

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
                ("uname", ["-a"]) => "Linux fake x86_64 GNU/Linux".into(),
                _ => bail!("unexpected command {program} {args:?}"),
            })
        }
        fn cpu(&self) -> String {
            "Fake CPU".into()
        }
        fn now(&self) -> Timestamp {
            "2026-10-10T04:00:00Z".parse().unwrap()
        }
    }

    const HEADER: &str = "arch,engine,engine_flags,wasm,phase,event,count\n";

    /// A scratch dir with the given `(file name, rows)` and `failures.toml`.
    fn scratch(files: &[(&str, &str)], failures: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        for (name, rows) in files {
            std::fs::write(tmp.path().join(name), format!("{HEADER}{rows}")).unwrap();
        }
        std::fs::write(tmp.path().join(FAILURES), failures).unwrap();
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
            "\"c/c.wasm\" = \"iteration 0: exit status: 1\"\n",
        )
    }

    fn options(scratch: &Path) -> Options {
        Options {
            scratch: scratch.to_path_buf(),
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
        let merged = merge(scratch.path()).unwrap();
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
            "",
        );
        assert!(merge(scratch.path()).is_err());
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
        assert_eq!(metadata.environment.cpu, "Fake CPU");
        assert_eq!(metadata.finished.to_string(), "2026-10-10T04:00:00Z");
        assert_eq!(metadata.config.iterations, 3);
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
            runs::write_csv_gz(&merge(scratch.path()).unwrap(), &mut out).unwrap();
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
        let scratch = scratch(&[], "\"a/a.wasm\" = \"boom\"\n");
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
