use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use core_api::{run_validation, ValidateRequest};

#[derive(Debug, Parser)]
#[command(name = "core-rs", version, about = "CDISC Rules Engine Rust port")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Print the CLI version.
    Version,
    /// Validate study data with CDISC rules.
    Validate(ValidateArgs),
}

#[derive(Debug, Parser)]
struct ValidateArgs {
    #[arg(short = 's', long, value_name = "TEXT")]
    standard: Option<String>,

    #[arg(short = 'v', long, value_name = "TEXT")]
    standard_version: Option<String>,

    #[arg(short = 'd', long, value_name = "DIR")]
    data: Option<PathBuf>,

    #[arg(long, value_name = "FILE", num_args = 1..)]
    dataset_path: Vec<PathBuf>,

    #[arg(long, value_name = "DIR_OR_FILE", num_args = 1..)]
    local_rules: Vec<PathBuf>,

    #[arg(short = 'r', long, value_name = "RULE_ID", num_args = 1..)]
    rules: Vec<String>,

    #[arg(long, value_name = "RULE_ID", num_args = 1..)]
    exclude_rules: Vec<String>,

    #[arg(short = 'o', long, value_name = "PATH")]
    output: Option<PathBuf>,

    #[arg(long, value_enum, value_name = "json|csv")]
    output_format: Option<OutputFormat>,

    #[arg(long, value_enum, value_name = "disabled|info|debug|warn|error")]
    log_level: Option<LogLevel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
    Json,
    Csv,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum LogLevel {
    Disabled,
    Info,
    Debug,
    Warn,
    Error,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Version => {
            println!("core-rs {}", env!("CARGO_PKG_VERSION"));
        }
        Commands::Validate(args) => {
            run_validate(args)?;
        }
    }

    Ok(())
}

fn run_validate(args: ValidateArgs) -> Result<()> {
    let mut dataset_paths = Vec::new();
    if let Some(data) = args.data {
        dataset_paths.push(data);
    }
    dataset_paths.extend(args.dataset_path);

    let outcome = run_validation(ValidateRequest {
        rule_paths: args.local_rules,
        dataset_paths,
        include_rules: args.rules,
        exclude_rules: args.exclude_rules,
        output_dir: args.output,
    })?;

    println!("validation completed: {} result(s)", outcome.results.len());
    if let Some(reports) = outcome.reports {
        println!("wrote {}", reports.json.display());
        println!("wrote {}", reports.csv.display());
    }

    Ok(())
}
