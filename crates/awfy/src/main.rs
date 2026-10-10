use anyhow::Result;
use awfy::metadata::Timestamp;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Continuous Sightglass benchmarking for Wasmtime and Cranelift.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Repository root, holding `config.toml`, `benchmark-data/`, and `site/`.
    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print a field from the latest run's metadata.
    ///
    /// Prints nothing if there are no runs.
    Latest {
        /// Dotted field, e.g. `wasmtime.commit` or `started`.
        field: String,
    },
    /// Run the suite under callgrind.
    Run {
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
        /// Units to run at once. Defaults to the available parallelism.
        #[arg(long)]
        jobs: Option<usize>,
    },
    /// Write a new run directory from `awfy run`'s output.
    ///
    /// Prints the new run number, e.g. `00042`.
    Record {
        /// `awfy run`'s output directory.
        #[arg(long)]
        scratch: PathBuf,
        /// Full SHA of the benchmarked Wasmtime commit.
        #[arg(long)]
        wasmtime_commit: String,
        /// Its committer date, RFC 3339.
        #[arg(long)]
        wasmtime_commit_date: Timestamp,
        /// When the run started, RFC 3339.
        #[arg(long)]
        started: Timestamp,
        /// URL of the workflow run.
        #[arg(long)]
        workflow_run: Option<String>,
        /// The suite passed to `awfy run --suite`, if any.
        #[arg(long)]
        suite: Option<String>,
    },
    /// Compare a run with its baseline.
    Regressions,
    /// Generate the static site.
    Site,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Latest { field } => awfy::latest::run(&cli.root, &field),
        Command::Run {
            engine,
            sightglass_cli,
            out,
            suite,
            jobs,
        } => awfy::run::run(
            &cli.root,
            awfy::run::Options {
                engine,
                sightglass_cli,
                out,
                suite,
                jobs: match jobs {
                    Some(jobs) => jobs,
                    None => std::thread::available_parallelism()?.get(),
                },
            },
        ),
        Command::Record {
            scratch,
            wasmtime_commit,
            wasmtime_commit_date,
            started,
            workflow_run,
            suite,
        } => awfy::record::run(
            &cli.root,
            awfy::record::Options {
                scratch,
                wasmtime_commit,
                wasmtime_commit_date,
                started,
                workflow_run,
                suite,
            },
        ),
        Command::Regressions => awfy::regressions::run(&cli.root),
        Command::Site => awfy::site::run(&cli.root),
    }
}
