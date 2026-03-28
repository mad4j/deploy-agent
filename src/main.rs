mod config;
mod executor;
mod logger;

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::process;

/// Execute deployment actions defined in a JSON configuration file.
///
/// Example:
///   deploy-agent --config deploy.json --verbose
#[derive(Parser, Debug)]
#[command(name = "deploy-agent", version, about)]
struct Cli {
    /// Path to the JSON configuration file.
    #[arg(short, long, default_value = "deploy.json")]
    config: PathBuf,

    /// Show what would be executed without actually running anything.
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Enable verbose output (show captured stdout, env changes, etc.).
    #[arg(short, long)]
    verbose: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {e:?}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let raw = std::fs::read_to_string(&cli.config)
        .with_context(|| format!("cannot read '{}'", cli.config.display()))?;

    let config: config::Config =
        serde_json::from_str(&raw).context("failed to parse JSON configuration")?;

    let mut executor = executor::Executor::new(cli.dry_run, cli.verbose);
    executor.run(&config)
}
