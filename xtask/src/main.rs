#![forbid(unsafe_code)]

mod open_rules;

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
}

#[derive(Debug, Parser)]
struct OpenRulesCommand {
    #[command(subcommand)]
    command: OpenRulesSubcommand,
}

#[derive(Debug, Subcommand)]
enum OpenRulesSubcommand {
    /// Score existing core-rust reports against official Open Rules results.
    Score(open_rules::ScoreArgs),
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let should_fail = match cli.command {
        Commands::OpenRules(command) => match command.command {
            OpenRulesSubcommand::Score(args) => open_rules::score(args)?,
        },
    };

    Ok(if should_fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
