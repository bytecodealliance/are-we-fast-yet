//! `awfy record`: turn `awfy run`'s scratch directory into a new, immutable
//! `benchmark-data/NNNNN/`. See `docs/data.md`.

use crate::config::Config;
use crate::metadata::{
    AreWeFastYet, Environment, Metadata, RunConfig, SCHEMA, Sightglass, Timestamp, Wasmtime,
};
use crate::run::FAILURES;
use crate::runs::{self, DataDir, RunNumber};
use anyhow::{Context, Result, bail};
use sightglass_data::Measurement;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub struct Options {
    pub scratch: PathBuf,
    pub wasmtime_commit: String,
    pub wasmtime_commit_date: Timestamp,
    pub started: Timestamp,
    pub workflow_run: Option<String>,
    /// The suite `awfy run` used, if not `config.toml`'s.
    pub suite: Option<String>,
}

/// Everything `record` learns from outside the scratch directory. Faked in
/// tests.
pub trait Probe {
    /// Trimmed stdout of a command that must succeed.
    fn command(&self, cwd: &Path, program: &str, args: &[&str]) -> Result<String>;
    /// The CPU model name.
    fn cpu(&self) -> String;
    fn now(&self) -> Timestamp;
}

pub struct SystemProbe;

impl Probe for SystemProbe {
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

pub fn run(root: &Path, options: Options) -> Result<()> {
    let run = record(root, &options, &SystemProbe)?;
    println!("{run}");
    Ok(())
}

/// Write the new run directory and return its number.
pub fn record(root: &Path, options: &Options, probe: &dyn Probe) -> Result<RunNumber> {
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
    std::fs::create_dir_all(&data.path)?;
    let run = data.next_run_number()?;
    let target = data.run_dir(run);
    if target.exists() {
        bail!("{} already exists", target.display());
    }

    // Build the run in a temp dir, then rename it into place, so a failure
    // never leaves a partial run. `runs()` ignores the temp dir's name.
    let tmp = tempfile::Builder::new()
        .prefix(".record-")
        .tempdir_in(&data.path)?;
    let csv = std::fs::File::create(tmp.path().join(runs::CSV))?;
    runs::write_csv_gz(&measurements, std::io::BufWriter::new(csv))?;

    let metadata = Metadata {
        schema: SCHEMA,
        started: options.started,
        finished: probe.now(),
        wasmtime: Wasmtime {
            commit: options.wasmtime_commit.clone(),
            commit_date: options.wasmtime_commit_date,
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
            processes: config.benchmark.processes,
            engine_flags: config.benchmark.engine_flags.clone(),
        },
        failures,
    };
    std::fs::write(tmp.path().join(runs::METADATA), metadata.to_toml()?)?;

    std::fs::rename(tmp.keep(), &target)
        .with_context(|| format!("renaming into {}", target.display()))?;
    log::info!(
        "recorded run {run}: {} rows, {} failed benchmark(s)",
        measurements.len(),
        metadata.failures.len()
    );
    Ok(run)
}

/// Read every per-unit CSV in `scratch`, normalize, and sort.
///
/// `process` becomes the unit's process index from its file name, since the
/// OS pid is noise. `engine` becomes `wasmtime`, and `wasm` is made relative
/// to `sightglass/benchmarks/`.
pub fn merge(scratch: &Path) -> Result<Vec<Measurement<'static>>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(scratch)
        .with_context(|| format!("reading {}", scratch.display()))?
        .map(|e| Ok(e?.path()))
        .collect::<Result<_>>()?;
    files.retain(|f| f.extension().is_some_and(|e| e == "csv"));
    files.sort();

    let mut measurements = Vec::new();
    for file in files {
        let process = process_index(&file)?;
        let rows = runs::read_csv(std::fs::File::open(&file)?)
            .with_context(|| format!("reading {}", file.display()))?;
        for mut m in rows {
            m.engine.name = "wasmtime".into();
            m.wasm = normalize_wasm(&m.wasm)
                .with_context(|| format!("in {}", file.display()))?
                .into();
            m.process = process;
            measurements.push(m);
        }
    }
    measurements.sort_by(|a, b| {
        (&a.wasm, a.process, a.iteration, a.phase, &a.event).cmp(&(
            &b.wasm,
            b.process,
            b.iteration,
            b.phase,
            &b.event,
        ))
    });
    Ok(measurements)
}

/// The process index from a unit file name such as `a__benchmark.wasm.002.csv`.
fn process_index(file: &Path) -> Result<u32> {
    file.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit_once('.'))
        .and_then(|(_, n)| n.parse().ok())
        .with_context(|| format!("no process index in file name {}", file.display()))
}

/// Make a wasm path relative to `sightglass/benchmarks/`: strip everything up
/// to and including the last `benchmarks` directory. A path without one must
/// already be relative to it.
pub fn normalize_wasm(wasm: &str) -> Result<String> {
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
    use std::cell::Cell;

    /// Canned answers. Fails `valgrind` when `fail_valgrind` is set.
    #[derive(Default)]
    struct FakeProbe {
        fail_valgrind: Cell<bool>,
    }

    impl Probe for FakeProbe {
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

    const HEADER: &str = "arch,engine,engine_flags,wasm,process,iteration,phase,event,count\n";

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
                    "x86_64,/tmp/libengine.so,-Cfoo,/home/r/sightglass/benchmarks/b/benchmark.wasm,4182,0,Execution,virtual-cycles,20\n\
                     x86_64,/tmp/libengine.so,-Cfoo,/home/r/sightglass/benchmarks/b/benchmark.wasm,4182,0,Compilation,virtual-cycles,10\n",
                ),
                (
                    "a__a.wasm.000.csv",
                    "x86_64,wasmtime,-Cfoo,a/a.wasm,77,0,Compilation,virtual-cycles,30\n\
                     x86_64,wasmtime,-Cfoo,a/a.wasm,77,0,Compilation,instructions-retired,15\n",
                ),
                (
                    "b__benchmark.wasm.000.csv",
                    "x86_64,wasmtime,-Cfoo,../../benchmarks/b/benchmark.wasm,99,0,Compilation,virtual-cycles,11\n",
                ),
            ],
            "\"c/c.wasm\" = \"process 0: exit status: 1\"\n",
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
                    "{} {} {} {} {} {} {}",
                    m.engine.name, m.wasm, m.process, m.iteration, m.phase, m.event, m.count
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                "wasmtime a/a.wasm 0 0 compilation instructions-retired 15",
                "wasmtime a/a.wasm 0 0 compilation virtual-cycles 30",
                "wasmtime b/benchmark.wasm 0 0 compilation virtual-cycles 11",
                "wasmtime b/benchmark.wasm 1 0 compilation virtual-cycles 10",
                "wasmtime b/benchmark.wasm 1 0 execution virtual-cycles 20",
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
                "x86_64,wasmtime,,/elsewhere/x.wasm,1,0,Execution,virtual-cycles,1\n",
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
        let run = record(root.path(), &options, &FakeProbe::default()).unwrap();
        assert_eq!(run, RunNumber(4));

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
        assert_eq!(metadata.config.processes, 3);
        assert_eq!(
            metadata.failures,
            [(
                "c/c.wasm".to_string(),
                "process 0: exit status: 1".to_string()
            )]
            .into()
        );

        let measurements = data.measurements(run).unwrap();
        assert_eq!(measurements.len(), 5);

        // Only the run directory was added.
        let mut entries: Vec<_> = std::fs::read_dir(&data.path)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        entries.sort();
        assert_eq!(entries, ["00001", "00003", "00004"]);
    }

    #[test]
    fn suite_defaults_to_config() {
        let root = fixture_root();
        let scratch = two_units();
        let run = record(root.path(), &options(scratch.path()), &FakeProbe::default()).unwrap();
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
        assert!(record(root.path(), &options(scratch.path()), &probe).is_err());
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
        let err = record(root.path(), &options(scratch.path()), &FakeProbe::default()).unwrap_err();
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
        assert!(record(root.path(), &options(scratch.path()), &FakeProbe::default()).is_err());
    }
}
