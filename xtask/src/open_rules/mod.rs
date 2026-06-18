pub mod discovery;
pub mod normalize;
pub mod report;
pub mod run;
pub mod score;
pub mod upstream;

pub use run::RunArgs;
pub use score::ScoreArgs;

pub fn run(args: RunArgs) -> anyhow::Result<bool> {
    run::run(args)
}

pub fn score(args: ScoreArgs) -> anyhow::Result<bool> {
    score::run(args)
}
