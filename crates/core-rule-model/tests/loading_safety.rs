use std::fs;

use core_rule_model::{load_rules_from_paths_with_warnings, RuleModelError};
use tempfile::tempdir;

const RULE: &str = r#"{
  "Core": {"Id": "CORE-PILOT-0001"},
  "Rule Type": "Record Data",
  "Check": {"name": "DOMAIN", "operator": "not_equal_to", "value": "AE"},
  "Outcome": {"Message": "Check DOMAIN"}
}"#;

#[test]
fn directory_duplicates_report_both_sources() {
    let dir = tempdir().unwrap();
    let first = dir.path().join("a.json");
    let second = dir.path().join("b.json");
    fs::write(&first, RULE).unwrap();
    fs::write(&second, RULE).unwrap();
    let error = load_rules_from_paths_with_warnings(&[dir.path().to_path_buf()]).unwrap_err();
    match error {
        RuleModelError::DuplicateRuleId {
            rule_id,
            first_path,
            second_path,
        } => {
            assert_eq!(rule_id, "CORE-PILOT-0001");
            assert_eq!(first_path, first);
            assert_eq!(second_path, second);
        }
        other => panic!("unexpected error: {other}"),
    }
}

#[test]
fn overlapping_directory_and_file_inputs_are_not_double_loaded() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rule.json");
    fs::write(&path, RULE).unwrap();
    for paths in [
        vec![dir.path().to_path_buf(), path.clone()],
        vec![path.clone(), dir.path().to_path_buf()],
        vec![path.clone(), path.clone()],
    ] {
        assert!(matches!(
            load_rules_from_paths_with_warnings(&paths),
            Err(RuleModelError::DuplicateRuleId { .. })
        ));
    }
}
