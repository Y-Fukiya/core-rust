use std::collections::BTreeSet;

use anyhow::{ensure, Context, Result};
use serde_json::Value;

use crate::open_rules::score::{ScoreSummary, Scoreboard};

pub(super) fn reject_duplicate_case_keys(label: &str, scoreboard: &Scoreboard) -> Result<()> {
    let mut keys = BTreeSet::new();
    for case in &scoreboard.cases {
        let key = super::case_key(case);
        ensure!(
            keys.insert(key.clone()),
            "{label} scoreboard has duplicate case key: {key}"
        );
    }
    Ok(())
}

pub(super) fn validate_scoreboard(label: &str, scoreboard: &Scoreboard) -> Result<()> {
    reject_duplicate_case_keys(label, scoreboard)?;
    let actual = serde_json::to_value(&scoreboard.summary)?;
    let expected = serde_json::to_value(ScoreSummary::from_cases(&scoreboard.cases))?;
    validate_summary_value(&actual, &expected, "summary")
        .with_context(|| format!("{label} scoreboard summary does not agree with cases"))
}

fn validate_summary_value(actual: &Value, expected: &Value, path: &str) -> Result<()> {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            for (key, value) in expected {
                validate_summary_value(&actual[key], value, &format!("{path}/{key}"))?;
            }
        }
        (Value::Array(actual), Value::Array(expected)) => {
            ensure!(
                actual.len() == expected.len(),
                "{path}: expected {} entries, found {}",
                expected.len(),
                actual.len()
            );
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                validate_summary_value(actual, expected, &format!("{path}/{index}"))?;
            }
        }
        (Value::Number(actual), Value::Number(expected))
            if actual.is_f64() && expected.is_f64() =>
        {
            // JSON round trips can change a ratio by a few ULPs; counts remain exact.
            ensure!(
                (actual.as_f64().unwrap() - expected.as_f64().unwrap()).abs() <= 4.0 * f64::EPSILON,
                "{path}: expected {expected}, found {actual}"
            );
        }
        _ => ensure!(
            actual == expected,
            "{path}: expected {expected}, found {actual}"
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_rules::baseline::{run, BaselineArgs};
    use crate::open_rules::score::ScoreBucket;

    fn fixture() -> Scoreboard {
        serde_json::from_str(include_str!("../../../../tests/open_rules/baseline.json")).unwrap()
    }

    fn run_pair(baseline: &Scoreboard, current: &Scoreboard) -> Result<bool> {
        let dir = tempfile::tempdir()?;
        let baseline_path = dir.path().join("baseline.json");
        let current_path = dir.path().join("current.json");
        serde_json::to_writer(std::fs::File::create(&baseline_path)?, baseline)?;
        serde_json::to_writer(std::fs::File::create(&current_path)?, current)?;
        run(BaselineArgs {
            baseline: baseline_path,
            scoreboard: current_path,
        })
    }

    #[test]
    fn baseline_cli_rejects_duplicate_keys_in_either_input_in_any_order() {
        let valid = fixture();
        for bad_baseline in [false, true] {
            for reverse in [false, true] {
                let mut invalid = valid.clone();
                let mut duplicate = invalid.cases[0].clone();
                duplicate.bucket = ScoreBucket::SupportedMismatch;
                invalid.cases.push(duplicate);
                if reverse {
                    invalid.cases.reverse();
                }
                invalid.summary = ScoreSummary::from_cases(&invalid.cases);
                let result = if bad_baseline {
                    run_pair(&invalid, &valid)
                } else {
                    run_pair(&valid, &invalid)
                };
                let error = result.expect_err("duplicate key must not be overwritten");
                let message = format!("{error:#}");
                assert!(message.contains("duplicate case key"), "{message}");
                assert!(
                    message.contains(if bad_baseline { "baseline" } else { "current" }),
                    "{message}"
                );
            }
        }
    }

    #[test]
    fn baseline_cli_rejects_stale_summary_in_either_input() {
        let valid = fixture();
        for field in [
            "total_cases",
            "supported_match",
            "coverage",
            "native_engine_coverage",
            "scoring_normalization_counts",
        ] {
            let mut json = serde_json::to_value(&valid).unwrap();
            json["summary"][field] = match field {
                "coverage" | "native_engine_coverage" => serde_json::json!(0.25),
                "scoring_normalization_counts" => {
                    serde_json::json!([{"normalization":"unrecorded", "cases":1}])
                }
                _ => serde_json::json!(99),
            };
            let invalid: Scoreboard = serde_json::from_value(json).unwrap();
            for result in [run_pair(&invalid, &valid), run_pair(&valid, &invalid)] {
                let error = result.expect_err("summary must agree with cases");
                assert!(format!("{error:#}").contains(field), "{error:#}");
            }
        }
        assert!(!run_pair(&valid, &valid).expect("valid inputs"));
    }

    #[test]
    fn committed_baselines_have_valid_case_keys_and_summaries() {
        for path in [
            "baseline.json",
            "curated-upstream-baseline.json",
            "curated-gap-baseline.json",
            "upstream-baseline.json",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/open_rules")
                .join(path);
            let board: Scoreboard =
                serde_json::from_reader(std::fs::File::open(&path).unwrap()).unwrap();
            validate_scoreboard(&path.display().to_string(), &board).unwrap();
        }
    }
}
