use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::yaml_literals::{
    normalize_yaml_condition_value_literals, yaml_condition_value_literals,
};
use crate::{
    normalize_rule, ExecutableRule, LoadRulesResult, LoadWarning, LoadWarningKind, Result,
    RuleModelError,
};

pub fn load_rule_file(path: impl AsRef<Path>) -> Result<ExecutableRule> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| RuleModelError::Io {
        path: path.to_path_buf(),
        source,
    })?;

    let value = match extension(path).as_deref() {
        Some("json") => {
            serde_json::from_str(&source).map_err(|source| RuleModelError::JsonParse {
                path: path.to_path_buf(),
                source,
            })?
        }
        Some("yaml" | "yml") => {
            let mut value: Value =
                serde_saphyr::from_str(&source).map_err(|source| RuleModelError::YamlParse {
                    path: path.to_path_buf(),
                    message: source.to_string(),
                })?;
            let mut value_literals = yaml_condition_value_literals(&source);
            normalize_yaml_condition_value_literals(&mut value, &mut value_literals)?;
            if !value_literals.is_empty() {
                return Err(RuleModelError::InvalidRuleFormat(
                    "YAML condition value literal normalization left unmatched scalar values"
                        .to_owned(),
                ));
            }
            value
        }
        Some(other) => return Err(RuleModelError::UnsupportedExtension(other.to_owned())),
        None => return Err(RuleModelError::UnsupportedExtension(String::new())),
    };

    normalize_rule(value)
}

pub fn load_rules_from_paths(paths: &[PathBuf]) -> Result<Vec<ExecutableRule>> {
    Ok(load_rules_from_paths_with_warnings(paths)?.rules)
}

pub fn load_rules_from_paths_with_warnings(paths: &[PathBuf]) -> Result<LoadRulesResult> {
    let mut rules = Vec::new();
    let mut warnings = Vec::new();

    for path in paths {
        if path.is_file() {
            if is_supported_rule_file(path) {
                rules.push(load_rule_file(path)?);
            } else {
                warnings.push(unsupported_extension_warning(path));
            }
        } else if path.is_dir() {
            let mut entries = fs::read_dir(path)
                .map_err(|source| RuleModelError::Io {
                    path: path.to_path_buf(),
                    source,
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|source| RuleModelError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                if path.is_file() && is_supported_rule_file(&path) {
                    rules.push(load_rule_file(path)?);
                } else if path.is_file() {
                    warnings.push(unsupported_extension_warning(&path));
                }
            }
        } else {
            return Err(RuleModelError::Io {
                path: path.clone(),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "path not found"),
            });
        }
    }

    Ok(LoadRulesResult { rules, warnings })
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}

fn is_supported_rule_file(path: &Path) -> bool {
    matches!(extension(path).as_deref(), Some("json" | "yaml" | "yml"))
}

fn unsupported_extension_warning(path: &Path) -> LoadWarning {
    LoadWarning {
        path: path.to_path_buf(),
        kind: LoadWarningKind::UnsupportedExtension(extension(path).unwrap_or_default()),
    }
}
