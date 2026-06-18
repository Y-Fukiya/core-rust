pub mod discovery;
pub mod normalize;
pub mod report;
pub mod run;
pub mod score;
pub mod upstream;

pub use run::{RunArgs, RunScoreArgs};
pub use score::ScoreArgs;

pub fn run(args: RunArgs) -> anyhow::Result<bool> {
    run::run(args)
}

pub fn run_score(args: RunScoreArgs) -> anyhow::Result<bool> {
    run::run_score(args)
}

pub fn score(args: ScoreArgs) -> anyhow::Result<bool> {
    score::run(args)
}
