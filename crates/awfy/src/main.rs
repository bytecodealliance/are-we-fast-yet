use anyhow::Result;
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
    Latest,
    /// Run the suite under callgrind.
    Run,
    /// Write a new run directory from `awfy run`'s output.
    Record,
    /// Compare a run with its baseline.
    Regressions,
    /// Generate the static site.
    Site,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cli = Cli::parse();
    match cli.command {
        Command::Latest => awfy::latest::run(&cli.root),
        Command::Run => awfy::run::run(&cli.root),
        Command::Record => awfy::record::run(&cli.root),
        Command::Regressions => awfy::regressions::run(&cli.root),
        Command::Site => awfy::site::run(&cli.root),
    }
}
