#![forbid(unsafe_code)]

mod open_rules;
mod release;

use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Development tasks for core-rust")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// CDISC Open Rules compatibility tooling.
    OpenRules(OpenRulesCommand),
    /// Write a release provenance manifest.
    ReleaseManifest(release::ReleaseManifestArgs),
}

#[derive(Debug, Parser)]
struct OpenRulesCommand {
    #[command(subcommand)]
    command: OpenRulesSubcommand,
}

#[derive(Debug, Subcommand)]
enum OpenRulesSubcommand {
    /// Compare an Open Rules scoreboard against an accepted baseline.
    Baseline(open_rules::BaselineArgs),
    /// Write a portable Open Rules baseline from a scoreboard.
    CanonicalizeBaseline(open_rules::CanonicalizeBaselineArgs),
    /// Run core-rust against Open Rules cases and write candidate reports.
    Run(open_rules::RunArgs),
    /// Run core-rust against Open Rules cases, then score candidate reports.
    RunScore(open_rules::RunScoreArgs),
    /// Score existing core-rust reports against official Open Rules results.
    Score(open_rules::ScoreArgs),
    /// Compare default and strict Open Rules scoreboards.
    ScoreDelta(open_rules::ScoreDeltaArgs),
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let should_fail = match cli.command {
        Commands::OpenRules(command) => match command.command {
            OpenRulesSubcommand::Baseline(args) => open_rules::baseline(args)?,
            OpenRulesSubcommand::CanonicalizeBaseline(args) => {
                open_rules::canonicalize_baseline(args)?
            }
            OpenRulesSubcommand::Run(args) => open_rules::run(args)?,
            OpenRulesSubcommand::RunScore(args) => open_rules::run_score(args)?,
            OpenRulesSubcommand::Score(args) => open_rules::score(args)?,
            OpenRulesSubcommand::ScoreDelta(args) => open_rules::score_delta(args)?,
        },
        Commands::ReleaseManifest(args) => release::run(args)?,
    };

    Ok(if should_fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
