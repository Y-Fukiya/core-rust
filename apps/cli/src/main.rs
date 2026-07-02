#![forbid(unsafe_code)]

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand, ValueEnum};
use core_api::{run_validation, ValidateRequest};
use core_engine::ExecutionStatus;
use core_report::ReportOutputFormat;

#[derive(Debug, Parser)]
#[command(name = "core-rs", version, about = "CDISC Rules Engine Rust port")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)]
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

    #[arg(long, value_name = "FILE", num_args = 1..)]
    define_xml: Vec<PathBuf>,

    #[arg(long, alias = "controlled-terminology", value_name = "FILE", num_args = 1..)]
    ct: Vec<PathBuf>,

    #[arg(long, alias = "dictionary", value_name = "FILE", num_args = 1..)]
    external_dictionary: Vec<PathBuf>,

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

    #[arg(long, value_enum, value_delimiter = ',', value_name = "failed|skipped")]
    fail_on: Vec<FailOnStatus>,

    #[arg(long)]
    strict: bool,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum FailOnStatus {
    Failed,
    Skipped,
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
        dataset_loader: Default::default(),
        open_rules_oracle_compat: false,
        define_xml_paths: args.define_xml,
        ct_paths: args.ct,
        external_dictionary_paths: args.external_dictionary,
        include_rules: args.rules,
        exclude_rules: args.exclude_rules,
        standard: args.standard,
        standard_version: args.standard_version,
        output_format: args
            .output_format
            .map(ReportOutputFormat::from)
            .unwrap_or_default(),
        log_level: args.log_level.map(|level| level.as_name().to_owned()),
        output_dir: args.output,
    })?;

    println!("validation completed: {} result(s)", outcome.results.len());
    if let Some(reports) = outcome.reports {
        if let Some(json) = reports.json {
            println!("wrote {}", json.display());
        }
        if let Some(csv) = reports.csv {
            println!("wrote {}", csv.display());
        }
        if let Some(log) = reports.log {
            println!("wrote {}", log.display());
        }
    }

    enforce_exit_policy(&outcome.results, args.strict, &args.fail_on)?;

    Ok(())
}

fn enforce_exit_policy(
    results: &[core_engine::RuleValidationResult],
    strict: bool,
    fail_on: &[FailOnStatus],
) -> Result<()> {
    let fail_on_failed = strict || fail_on.contains(&FailOnStatus::Failed);
    let fail_on_skipped = strict || fail_on.contains(&FailOnStatus::Skipped);
    if !fail_on_failed && !fail_on_skipped {
        return Ok(());
    }

    let failed = results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Failed)
        .count();
    let skipped = results
        .iter()
        .filter(|result| result.execution_status == ExecutionStatus::Skipped)
        .count();
    if (fail_on_failed && failed > 0) || (fail_on_skipped && skipped > 0) {
        bail!("validation failed strict exit policy: failed={failed}, skipped={skipped}");
    }
    Ok(())
}

impl From<OutputFormat> for ReportOutputFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Json => Self::Json,
            OutputFormat::Csv => Self::Csv,
        }
    }
}

impl LogLevel {
    fn as_name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}
