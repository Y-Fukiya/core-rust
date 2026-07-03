pub mod baseline;
pub mod discovery;
pub mod normalize;
pub mod report;
pub mod run;
pub mod score;
pub mod score_delta;
pub mod upstream;

pub use baseline::{BaselineArgs, CanonicalizeBaselineArgs};
pub use run::{RunArgs, RunScoreArgs};
pub use score::ScoreArgs;
pub use score_delta::ScoreDeltaArgs;

pub fn run(args: RunArgs) -> anyhow::Result<bool> {
    run::run(args)
}

pub fn run_score(args: RunScoreArgs) -> anyhow::Result<bool> {
    run::run_score(args)
}

pub fn score(args: ScoreArgs) -> anyhow::Result<bool> {
    score::run(args)
}

pub fn score_delta(args: ScoreDeltaArgs) -> anyhow::Result<bool> {
    score_delta::run(args)
}

pub fn baseline(args: BaselineArgs) -> anyhow::Result<bool> {
    baseline::run(args)
}

pub fn canonicalize_baseline(args: CanonicalizeBaselineArgs) -> anyhow::Result<bool> {
    baseline::canonicalize(args)
}
