use anyhow::Result;
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
    Run(awfy::run::Options),
    /// Write a new run directory from `awfy run`'s output.
    ///
    /// Prints the new run number, e.g. `00042`.
    Record(awfy::record::Options),
    /// Compare a run with its baseline.
    ///
    /// Writes a Markdown job summary. With `--issue-dir`, also writes
    /// `title.txt` and `body.md` there if there are regressions.
    Regressions(awfy::regressions::Options),
    /// Print the title suffix that identifies a run's regression issue.
    IssueKey {
        #[arg(long)]
        run: RunNumber,
    },
    /// Check that every run in `benchmark-data/` and `excluded.toml` parse.
    LintData,
    /// Generate the static site: copy `site/` and write JSON to `data/`.
    Site(awfy::site::Options),
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
        Command::Run(options) => awfy::run::run(&cli.root, options),
        Command::Record(options) => awfy::record::run(&cli.root, options),
        Command::Regressions(options) => awfy::regressions::run(&cli.root, options),
        Command::IssueKey { run } => {
            println!("{}", awfy::issue::issue_key(run));
            Ok(())
        }
        Command::LintData => awfy::lint::run(&cli.root),
        Command::Site(options) => awfy::site::run(&cli.root, options),
    }
}
