use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use core_rule_model::{
    load_rule_file, normalize_key, ConditionGroup, Operator, RuleModelError, RuleType,
};
use serde::Serialize;

type AuditResult<T> = Result<T, Box<dyn std::error::Error>>;
type Index = BTreeMap<String, BTreeSet<String>>;

fn index(index: &mut Index, key: &str, value: &str) {
    index
        .entry(key.to_owned())
        .or_default()
        .insert(value.to_owned());
}

#[derive(Default, Serialize)]
struct Counts {
    discovered: usize,
    parsed: usize,
    parse_errors: usize,
    duplicate_ids: usize,
}

#[derive(Default, Serialize)]
struct Audit {
    scope: &'static str,
    engine_execution_performed: bool,
    v1_conformance_assessed: bool,
    counts: Counts,
    records: Vec<AuditRecord>,
    duplicate_ids: Index,
    multiple_definitions_by_directory: Index,
    unresolved_operator_rule_ids: Index,
    unresolved_rule_type_rule_ids: Index,
    skipped_symlinks: BTreeSet<String>,
}

#[derive(Serialize)]
struct AuditRecord {
    relative_path: String,
    core_id: Option<String>,
    #[serde(flatten)]
    result: ParseResult,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ParseResult {
    ParseError {
        error: String,
    },
    Parsed {
        rule_type: RuleType,
        condition_shape: ConditionShape,
        unsupported_operators: BTreeSet<String>,
        unsupported_rule_type: Option<String>,
        operation_names_observed: Index,
    },
}

#[derive(Default, Serialize)]
struct ConditionShape {
    all: usize,
    any: usize,
    not: usize,
    leaves: usize,
}

fn inspect(group: &ConditionGroup, shape: &mut ConditionShape, unsupported: &mut BTreeSet<String>) {
    match group {
        ConditionGroup::All(children) | ConditionGroup::Any(children) => {
            if matches!(group, ConditionGroup::All(_)) {
                shape.all += 1;
            } else {
                shape.any += 1;
            }
            for child in children {
                inspect(child, shape, unsupported);
            }
        }
        ConditionGroup::Not(child) => {
            shape.not += 1;
            inspect(child, shape, unsupported);
        }
        ConditionGroup::Leaf(condition) => {
            shape.leaves += 1;
            if let Operator::Unsupported(name) = &condition.operator {
                unsupported.insert(name.clone());
            }
        }
    }
}

fn relative(root: &Path, path: &Path) -> AuditResult<String> {
    Ok(path
        .strip_prefix(root)?
        .to_str()
        .ok_or("non-UTF-8 path in rules tree")?
        .replace(std::path::MAIN_SEPARATOR, "/"))
}

fn discover(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
    skipped: &mut BTreeSet<String>,
) -> AuditResult<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            skipped.insert(relative(root, &path)?);
        } else if kind.is_dir() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if !matches!(name.as_str(), "positive" | "negative") {
                discover(root, &path, files, skipped)?;
            }
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("yml") || s.eq_ignore_ascii_case("yaml"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn audit(root: &Path) -> AuditResult<Audit> {
    if !root.is_dir() {
        return Err("rules root must be an existing directory".into());
    }
    let root = root.canonicalize()?;
    let mut files = Vec::new();
    let mut report = Audit {
        scope: "parser audit only",
        engine_execution_performed: false,
        v1_conformance_assessed: false,
        ..Audit::default()
    };
    discover(&root, &root, &mut files, &mut report.skipped_symlinks)?;
    files.sort();
    if files.is_empty() {
        return Err(
            "rules root contains no YAML definitions outside positive/negative subtrees".into(),
        );
    }
    report.counts.discovered = files.len();
    for path in files {
        let relative_path = relative(&root, &path)?;
        let directory = relative(&root, path.parent().unwrap())?;
        index(
            &mut report.multiple_definitions_by_directory,
            &directory,
            &relative_path,
        );
        let (core_id, result) = match load_rule_file(&path) {
            Err(error) => {
                report.counts.parse_errors += 1;
                let error = match error {
                    RuleModelError::YamlParse { message, .. } => message,
                    RuleModelError::JsonParse { source, .. } => source.to_string(),
                    RuleModelError::Io { source, .. } => source.to_string(),
                    other => other.to_string(),
                };
                (None, ParseResult::ParseError { error })
            }
            Ok(rule) => {
                report.counts.parsed += 1;
                index(&mut report.duplicate_ids, &rule.core_id, &relative_path);
                let mut condition_shape = ConditionShape::default();
                let mut unsupported_operators = BTreeSet::new();
                inspect(
                    &rule.conditions,
                    &mut condition_shape,
                    &mut unsupported_operators,
                );
                for name in &unsupported_operators {
                    index(
                        &mut report.unresolved_operator_rule_ids,
                        name,
                        &rule.core_id,
                    );
                }
                let unsupported_rule_type = if let RuleType::Unsupported(name) = &rule.rule_type {
                    index(
                        &mut report.unresolved_rule_type_rule_ids,
                        name,
                        &rule.core_id,
                    );
                    Some(name.clone())
                } else {
                    None
                };
                let mut operation_names_observed = Index::new();
                for operation in &rule.operations {
                    for (key, value) in &operation.fields {
                        let key = normalize_key(key);
                        if matches!(key.as_str(), "name" | "operator" | "type") {
                            if let Some(name) = value.as_str() {
                                index(&mut operation_names_observed, &key, name);
                            }
                        }
                    }
                }
                (
                    Some(rule.core_id),
                    ParseResult::Parsed {
                        rule_type: rule.rule_type,
                        condition_shape,
                        unsupported_operators,
                        unsupported_rule_type,
                        operation_names_observed,
                    },
                )
            }
        };
        report.records.push(AuditRecord {
            relative_path,
            core_id,
            result,
        });
    }
    report.duplicate_ids.retain(|_, paths| paths.len() > 1);
    report
        .multiple_definitions_by_directory
        .retain(|_, paths| paths.len() > 1);
    report.counts.duplicate_ids = report.duplicate_ids.len();
    Ok(report)
}

fn run() -> AuditResult<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("usage: audit_rule_package <rules-root>".into());
    }
    let report = audit(Path::new(&args[0]))?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("audit_rule_package: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn write(root: &Path, path: &str, text: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn rule(id: &str, check: Value) -> String {
        json!({"Core": {"Id": id}, "Rule Type": "Record Data", "Check": check,
            "Outcome": {"Message": "test"}})
        .to_string()
    }

    #[test]
    fn rejects_missing_empty_and_file_roots() {
        let dir = tempfile::tempdir().unwrap();
        assert!(audit(&dir.path().join("missing")).is_err());
        assert!(audit(dir.path()).is_err());
        write(dir.path(), "file.yml", "bad");
        assert!(audit(&dir.path().join("file.yml")).is_err());
    }

    #[test]
    fn inventories_arbitrary_yaml_duplicates_errors_and_ambiguity_deterministically() {
        let dir = tempfile::tempdir().unwrap();
        let source = rule("CORE-1", json!({"operator": "exists"}));
        for path in ["b/unusual.yaml", "a/another.yml", "a/first.YML"] {
            write(dir.path(), path, &source);
        }
        write(dir.path(), "c/broken.yml", "Core: [");
        for path in ["a/positive/ignored.yml", "a/negative/nested/ignored.yaml"] {
            write(dir.path(), path, "bad");
        }
        let value = serde_json::to_value(audit(dir.path()).unwrap()).unwrap();
        assert_eq!(
            value["counts"],
            json!({"discovered": 4, "parsed": 3, "parse_errors": 1, "duplicate_ids": 1})
        );
        assert_eq!(
            value["duplicate_ids"]["CORE-1"],
            json!(["a/another.yml", "a/first.YML", "b/unusual.yaml"])
        );
        assert_eq!(
            value["multiple_definitions_by_directory"]["a"],
            json!(["a/another.yml", "a/first.YML"])
        );
        assert_eq!(value["records"][3]["status"], "parse_error");
        assert!(value["records"][3]["core_id"].is_null());
        assert!(!value["records"][3]["error"]
            .as_str()
            .unwrap()
            .contains(dir.path().to_str().unwrap()));
        assert_eq!(
            value,
            serde_json::to_value(audit(dir.path()).unwrap()).unwrap()
        );
        assert_eq!(value["engine_execution_performed"], false);
        assert_eq!(value["v1_conformance_assessed"], false);
    }

    #[test]
    fn walks_only_ast_including_all_any_not_and_observes_operations() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "CORE-1/arbitrary.yml",
            r#"
Core: {Id: CORE-1}
Rule Type: Future Type
Description: {operator: metadata_fake}
Check:
  all:
    - any:
        - not: {operator: future_op}
        - {operator: future_op}
    - operator: equal_to
      value_is_literal: true
      value: {operator: literal_fake}
      metadata: {operator: option_fake}
Operations:
  - {name: result_name, operator: future_operation}
Outcome: {Message: test}
"#,
        );
        let value = serde_json::to_value(audit(dir.path()).unwrap()).unwrap();
        let record = &value["records"][0];
        assert_eq!(
            record["condition_shape"],
            json!({"all": 1, "any": 1, "not": 1, "leaves": 3})
        );
        assert_eq!(record["unsupported_operators"], json!(["future_op"]));
        assert_eq!(record["unsupported_rule_type"], "Future Type");
        assert_eq!(
            record["operation_names_observed"],
            json!({"name": ["result_name"], "operator": ["future_operation"]})
        );
        assert_eq!(
            value["unresolved_operator_rule_ids"],
            json!({"future_op": ["CORE-1"]})
        );
        assert_eq!(
            value["unresolved_rule_type_rule_ids"],
            json!({"Future Type": ["CORE-1"]})
        );
    }

    #[cfg(unix)]
    #[test]
    fn skips_external_file_directory_and_cycle_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(
            outside.path(),
            "external.yml",
            &rule("EXTERNAL", json!({"operator": "exists"})),
        );
        write(
            dir.path(),
            "local.yml",
            &rule("LOCAL", json!({"operator": "exists"})),
        );
        for (target, name) in [
            (outside.path().join("external.yml"), "link.yml"),
            (outside.path().to_path_buf(), "outside"),
            (dir.path().to_path_buf(), "cycle"),
        ] {
            std::os::unix::fs::symlink(target, dir.path().join(name)).unwrap();
        }
        let report = audit(dir.path()).unwrap();
        assert_eq!(report.counts.discovered, 1);
        assert_eq!(
            report.skipped_symlinks,
            BTreeSet::from(["cycle".into(), "link.yml".into(), "outside".into()])
        );
    }
}
