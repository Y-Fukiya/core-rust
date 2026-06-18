pub mod discovery;
pub mod normalize;
pub mod report;
pub mod score;
pub mod upstream;

pub use score::ScoreArgs;

pub fn score(args: ScoreArgs) -> anyhow::Result<bool> {
    score::run(args)
}
