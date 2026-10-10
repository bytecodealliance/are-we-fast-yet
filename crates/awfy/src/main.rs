use anyhow::Result;
use awfy::metadata::Timestamp;
use awfy::runs::RunNumber;
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
    /// Print a value from `config.toml`, e.g. `regressions.issue-repo`.
    Config {
        /// Dotted key.
        key: String,
    },
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
        /// Processes per benchmark instead of `config.toml`'s. Recorded in
        /// metadata by `awfy record --processes`.
        #[arg(long)]
        processes: Option<u32>,
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
        /// The processes passed to `awfy run --processes`, if any.
        #[arg(long)]
        processes: Option<u32>,
    },
    /// Compare a run with its baseline.
    ///
    /// Writes a Markdown job summary. With `--issue-dir`, also writes
    /// `title.txt` and `body.md` there if there are regressions.
    Regressions {
        /// The run to check, e.g. `00042` or `42`.
        #[arg(long)]
        run: RunNumber,
        /// Append the job summary to this file instead of stderr, e.g.
        /// `$GITHUB_STEP_SUMMARY`.
        #[arg(long)]
        summary: Option<PathBuf>,
        /// Write the issue title and body here, if there are regressions.
        #[arg(long)]
        issue_dir: Option<PathBuf>,
        /// A Wasmtime clone, for the issue's commit log.
        #[arg(long)]
        wasmtime_repo: Option<PathBuf>,
    },
    /// Print the title suffix that identifies a run's regression issue.
    IssueKey {
        #[arg(long)]
        run: RunNumber,
    },
    /// Check that every run in `benchmark-data/` and `excluded.toml` parse.
    LintData,
    /// Generate the static site: copy `site/` and write JSON to `data/`.
    Site {
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
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Config { key } => {
            println!("{}", awfy::config::get(&cli.root, &key)?);
            Ok(())
        }
        Command::Latest { field } => awfy::latest::run(&cli.root, &field),
        Command::Run {
            engine,
            sightglass_cli,
            out,
            suite,
            processes,
            jobs,
        } => awfy::run::run(
            &cli.root,
            awfy::run::Options {
                engine,
                sightglass_cli,
                out,
                suite,
                processes,
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
            processes,
        } => awfy::record::run(
            &cli.root,
            awfy::record::Options {
                scratch,
                wasmtime_commit,
                wasmtime_commit_date,
                started,
                workflow_run,
                suite,
                processes,
            },
        ),
        Command::Regressions {
            run,
            summary,
            issue_dir,
            wasmtime_repo,
        } => awfy::regressions::run(
            &cli.root,
            awfy::regressions::Options {
                run,
                summary,
                issue_dir,
                wasmtime_repo,
            },
        ),
        Command::IssueKey { run } => {
            println!("{}", awfy::issue::issue_key(run));
            Ok(())
        }
        Command::LintData => awfy::lint::run(&cli.root),
        Command::Site { out, site, now } => {
            awfy::site::run(&cli.root, awfy::site::Options { out, site, now })
        }
    }
}
