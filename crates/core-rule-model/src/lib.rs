#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, RuleModelError>;

#[derive(Debug, Error)]
pub enum RuleModelError {
    #[error("unsupported rule file extension: {0}")]
    UnsupportedExtension(String),
    #[error("failed to read file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse JSON rule {path}: {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to parse YAML rule {path}: {message}")]
    YamlParse { path: PathBuf, message: String },
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("invalid rule format: {0}")]
    InvalidRuleFormat(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutableRule {
    pub core_id: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub sensitivity: Option<Sensitivity>,
    #[serde(default)]
    pub executability: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub authorities: Vec<Value>,
    #[serde(default)]
    pub standards: Vec<StandardRef>,
    #[serde(default)]
    pub classes: Option<Value>,
    #[serde(default)]
    pub domains: Option<Value>,
    #[serde(default)]
    pub datasets: Option<Vec<MatchDataset>>,
    #[serde(default)]
    pub entities: Option<Value>,
    pub rule_type: RuleType,
    pub conditions: ConditionGroup,
    #[serde(default)]
    pub actions: Vec<ActionSpec>,
    #[serde(default)]
    pub operations: Vec<OperationSpec>,
    #[serde(default)]
    pub output_variables: Vec<String>,
    #[serde(default)]
    pub grouping_variables: Vec<String>,
    #[serde(default)]
    pub use_case: Option<String>,
    #[serde(default)]
    pub status: Option<RuleStatus>,
    #[serde(default)]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConditionGroup {
    All(Vec<ConditionGroup>),
    Any(Vec<ConditionGroup>),
    Not(Box<ConditionGroup>),
    Leaf(Condition),
}

impl Serialize for ConditionGroup {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            ConditionGroup::All(conditions) => map.serialize_entry("all", conditions)?,
            ConditionGroup::Any(conditions) => map.serialize_entry("any", conditions)?,
            ConditionGroup::Not(condition) => map.serialize_entry("not", condition)?,
            ConditionGroup::Leaf(condition) => map.serialize_entry("leaf", condition)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ConditionGroup {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        deserialize_condition_group_value(value).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Condition {
    #[serde(default)]
    pub target: Option<String>,
    pub operator: Operator,
    #[serde(default)]
    pub comparator: ValueExpr,
    #[serde(default)]
    pub options: OperatorOptions,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum ValueExpr {
    Literal(Value),
    ColumnRef(String),
    List(Vec<Value>),
    #[default]
    Null,
}

impl Serialize for ValueExpr {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ValueExpr::Literal(value) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("literal", value)?;
                map.end()
            }
            ValueExpr::ColumnRef(column) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("column_ref", column)?;
                map.end()
            }
            ValueExpr::List(values) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("list", values)?;
                map.end()
            }
            ValueExpr::Null => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for ValueExpr {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(value_expr_from_value(Value::deserialize(deserializer)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OperatorOptions {
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Exists,
    NotExists,
    EqualTo,
    NotEqualTo,
    EqualToCaseInsensitive,
    NotEqualToCaseInsensitive,
    Contains,
    DoesNotContain,
    ContainsCaseInsensitive,
    DoesNotContainCaseInsensitive,
    IsContainedBy,
    IsNotContainedBy,
    IsContainedByCaseInsensitive,
    IsNotContainedByCaseInsensitive,
    LessThan,
    LessThanOrEqualTo,
    GreaterThan,
    GreaterThanOrEqualTo,
    MatchesRegex,
    DoesNotMatchRegex,
    DoesNotMatchRegexFullString,
    LongerThan,
    StartsWith,
    EndsWith,
    SuffixMatchesRegex,
    NotSuffixMatchesRegex,
    DateEqualTo,
    DateLessThan,
    DateLessThanOrEqualTo,
    DateGreaterThan,
    DateGreaterThanOrEqualTo,
    InvalidDate,
    InvalidDuration,
    IsCompleteDate,
    IsIncompleteDate,
    TargetIsNotSortedBy,
    EmptyWithinExceptLastRow,
    DoesNotHaveNextCorrespondingRecord,
    NotPresentOnMultipleRowsWithin,
    InconsistentEnumeratedColumns,
    IsNotUniqueSet,
    IsUniqueSet,
    IsNotUniqueRelationship,
    IsInconsistentAcrossDataset,
    DoesNotEqualStringPart,
    IsEmpty,
    IsNotEmpty,
    Unsupported(String),
}

impl Operator {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "exists" => Self::Exists,
            "not_exists" => Self::NotExists,
            "equal_to" => Self::EqualTo,
            "not_equal_to" => Self::NotEqualTo,
            "equal_to_case_insensitive" => Self::EqualToCaseInsensitive,
            "not_equal_to_case_insensitive" => Self::NotEqualToCaseInsensitive,
            "contains" => Self::Contains,
            "does_not_contain" => Self::DoesNotContain,
            "contains_case_insensitive" => Self::ContainsCaseInsensitive,
            "does_not_contain_case_insensitive" => Self::DoesNotContainCaseInsensitive,
            "is_contained_by" => Self::IsContainedBy,
            "is_not_contained_by" => Self::IsNotContainedBy,
            "is_contained_by_case_insensitive" => Self::IsContainedByCaseInsensitive,
            "is_not_contained_by_case_insensitive" => Self::IsNotContainedByCaseInsensitive,
            "less_than" => Self::LessThan,
            "less_than_or_equal_to" => Self::LessThanOrEqualTo,
            "greater_than" => Self::GreaterThan,
            "greater_than_or_equal_to" => Self::GreaterThanOrEqualTo,
            "matches_regex" => Self::MatchesRegex,
            "does_not_match_regex" => Self::DoesNotMatchRegex,
            "not_matches_regex" => Self::DoesNotMatchRegexFullString,
            "longer_than" => Self::LongerThan,
            "starts_with" => Self::StartsWith,
            "ends_with" => Self::EndsWith,
            "suffix_matches_regex" => Self::SuffixMatchesRegex,
            "not_suffix_matches_regex" => Self::NotSuffixMatchesRegex,
            "date_equal_to" => Self::DateEqualTo,
            "date_less_than" => Self::DateLessThan,
            "date_less_than_or_equal_to" => Self::DateLessThanOrEqualTo,
            "date_greater_than" => Self::DateGreaterThan,
            "date_greater_than_or_equal_to" => Self::DateGreaterThanOrEqualTo,
            "invalid_date" => Self::InvalidDate,
            "invalid_duration" => Self::InvalidDuration,
            "is_complete_date" => Self::IsCompleteDate,
            "is_incomplete_date" => Self::IsIncompleteDate,
            "target_is_not_sorted_by" => Self::TargetIsNotSortedBy,
            "empty_within_except_last_row" => Self::EmptyWithinExceptLastRow,
            "does_not_have_next_corresponding_record" => Self::DoesNotHaveNextCorrespondingRecord,
            "not_present_on_multiple_rows_within" => Self::NotPresentOnMultipleRowsWithin,
            "inconsistent_enumerated_columns" => Self::InconsistentEnumeratedColumns,
            "is_not_unique_set" => Self::IsNotUniqueSet,
            "is_unique_set" => Self::IsUniqueSet,
            "is_not_unique_relationship" => Self::IsNotUniqueRelationship,
            "is_inconsistent_across_dataset" => Self::IsInconsistentAcrossDataset,
            "does_not_equal_string_part" => Self::DoesNotEqualStringPart,
            "is_empty" | "empty" => Self::IsEmpty,
            "is_not_empty" | "non_empty" => Self::IsNotEmpty,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Exists => "exists",
            Self::NotExists => "not_exists",
            Self::EqualTo => "equal_to",
            Self::NotEqualTo => "not_equal_to",
            Self::EqualToCaseInsensitive => "equal_to_case_insensitive",
            Self::NotEqualToCaseInsensitive => "not_equal_to_case_insensitive",
            Self::Contains => "contains",
            Self::DoesNotContain => "does_not_contain",
            Self::ContainsCaseInsensitive => "contains_case_insensitive",
            Self::DoesNotContainCaseInsensitive => "does_not_contain_case_insensitive",
            Self::IsContainedBy => "is_contained_by",
            Self::IsNotContainedBy => "is_not_contained_by",
            Self::IsContainedByCaseInsensitive => "is_contained_by_case_insensitive",
            Self::IsNotContainedByCaseInsensitive => "is_not_contained_by_case_insensitive",
            Self::LessThan => "less_than",
            Self::LessThanOrEqualTo => "less_than_or_equal_to",
            Self::GreaterThan => "greater_than",
            Self::GreaterThanOrEqualTo => "greater_than_or_equal_to",
            Self::MatchesRegex => "matches_regex",
            Self::DoesNotMatchRegex => "does_not_match_regex",
            Self::DoesNotMatchRegexFullString => "not_matches_regex",
            Self::LongerThan => "longer_than",
            Self::StartsWith => "starts_with",
            Self::EndsWith => "ends_with",
            Self::SuffixMatchesRegex => "suffix_matches_regex",
            Self::NotSuffixMatchesRegex => "not_suffix_matches_regex",
            Self::DateEqualTo => "date_equal_to",
            Self::DateLessThan => "date_less_than",
            Self::DateLessThanOrEqualTo => "date_less_than_or_equal_to",
            Self::DateGreaterThan => "date_greater_than",
            Self::DateGreaterThanOrEqualTo => "date_greater_than_or_equal_to",
            Self::InvalidDate => "invalid_date",
            Self::InvalidDuration => "invalid_duration",
            Self::IsCompleteDate => "is_complete_date",
            Self::IsIncompleteDate => "is_incomplete_date",
            Self::TargetIsNotSortedBy => "target_is_not_sorted_by",
            Self::EmptyWithinExceptLastRow => "empty_within_except_last_row",
            Self::DoesNotHaveNextCorrespondingRecord => "does_not_have_next_corresponding_record",
            Self::NotPresentOnMultipleRowsWithin => "not_present_on_multiple_rows_within",
            Self::InconsistentEnumeratedColumns => "inconsistent_enumerated_columns",
            Self::IsNotUniqueSet => "is_not_unique_set",
            Self::IsUniqueSet => "is_unique_set",
            Self::IsNotUniqueRelationship => "is_not_unique_relationship",
            Self::IsInconsistentAcrossDataset => "is_inconsistent_across_dataset",
            Self::DoesNotEqualStringPart => "does_not_equal_string_part",
            Self::IsEmpty => "is_empty",
            Self::IsNotEmpty => "is_not_empty",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for Operator {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for Operator {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleType {
    RecordData,
    DatasetMetadata,
    VariableMetadata,
    DomainPresence,
    ValueLevelMetadata,
    Jsonata,
    Unsupported(String),
}

impl RuleType {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "record_data" => Self::RecordData,
            "dataset_metadata" => Self::DatasetMetadata,
            "variable_metadata" => Self::VariableMetadata,
            "domain_presence" => Self::DomainPresence,
            "value_level_metadata" => Self::ValueLevelMetadata,
            "jsonata" => Self::Jsonata,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::RecordData => "record_data",
            Self::DatasetMetadata => "dataset_metadata",
            Self::VariableMetadata => "variable_metadata",
            Self::DomainPresence => "domain_presence",
            Self::ValueLevelMetadata => "value_level_metadata",
            Self::Jsonata => "jsonata",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for RuleType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for RuleType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sensitivity {
    Record,
    Dataset,
    Group,
    Study,
    Unsupported(String),
}

impl Sensitivity {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "record" => Self::Record,
            "dataset" => Self::Dataset,
            "group" => Self::Group,
            "study" => Self::Study,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Record => "record",
            Self::Dataset => "dataset",
            Self::Group => "group",
            Self::Study => "study",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for Sensitivity {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for Sensitivity {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ActionSpec {
    pub name: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StandardRef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct MatchDataset {
    #[serde(default, flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct OperationSpec {
    #[serde(default, flatten)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RuleStatus {
    Published,
    Draft,
    Retired,
    Disabled,
    Unsupported(String),
}

impl RuleStatus {
    pub fn from_name(name: impl AsRef<str>) -> Self {
        let original = name.as_ref();
        match normalize_name(original).as_str() {
            "published" => Self::Published,
            "draft" => Self::Draft,
            "retired" => Self::Retired,
            "disabled" => Self::Disabled,
            _ => Self::Unsupported(original.to_owned()),
        }
    }

    pub fn as_name(&self) -> &str {
        match self {
            Self::Published => "published",
            Self::Draft => "draft",
            Self::Retired => "retired",
            Self::Disabled => "disabled",
            Self::Unsupported(name) => name.as_str(),
        }
    }
}

impl Serialize for RuleStatus {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_name())
    }
}

impl<'de> Deserialize<'de> for RuleStatus {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Ok(Self::from_name(name))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadWarning {
    pub path: PathBuf,
    pub kind: LoadWarningKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadWarningKind {
    UnsupportedExtension(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadRulesResult {
    pub rules: Vec<ExecutableRule>,
    pub warnings: Vec<LoadWarning>,
}

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
            let source = quote_yaml_value_literals(&source);
            serde_saphyr::from_str(&source).map_err(|source| RuleModelError::YamlParse {
                path: path.to_path_buf(),
                message: source.to_string(),
            })?
        }
        Some(other) => return Err(RuleModelError::UnsupportedExtension(other.to_owned())),
        None => return Err(RuleModelError::UnsupportedExtension(String::new())),
    };

    normalize_rule(value)
}

fn quote_yaml_value_literals(source: &str) -> String {
    let mut in_value_list_indent = None;
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let indent_len = line.len() - trimmed.len();

            if let Some(value_indent) = in_value_list_indent {
                if trimmed.is_empty() {
                    return line.to_owned();
                }
                if indent_len <= value_indent {
                    in_value_list_indent = None;
                } else if trimmed.starts_with("- ") {
                    return quote_yaml_value_list_item(line);
                }
            }

            if is_empty_yaml_value_line(line) {
                in_value_list_indent = Some(indent_len);
                line.to_owned()
            } else {
                quote_yaml_value_literal_line(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn quote_yaml_value_literal_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("value:") else {
        return line.to_owned();
    };
    let scalar = rest.trim();
    if !is_yaml_boolish_string(scalar) {
        return line.to_owned();
    }
    let indent_len = line.len() - trimmed.len();
    format!("{}value: \"{}\"", &line[..indent_len], scalar)
}

fn is_empty_yaml_value_line(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("value:")
        .is_some_and(|rest| rest.trim().is_empty())
}

fn quote_yaml_value_list_item(line: &str) -> String {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix("- ") else {
        return line.to_owned();
    };
    let scalar = rest.trim();
    if !is_yaml_boolish_string(scalar) {
        return line.to_owned();
    }
    let indent_len = line.len() - trimmed.len();
    format!("{}- \"{}\"", &line[..indent_len], scalar)
}

fn is_yaml_boolish_string(value: &str) -> bool {
    matches!(
        value,
        "Y" | "y" | "N" | "n" | "Yes" | "yes" | "YES" | "No" | "no" | "NO"
    )
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
                    path: path.clone(),
                    source,
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|source| RuleModelError::Io {
                    path: path.clone(),
                    source,
                })?;

            entries.sort_by_key(|entry| entry.path());

            for entry in entries {
                let child = entry.path();
                if !child.is_file() {
                    continue;
                }

                if is_supported_rule_file(&child) {
                    rules.push(load_rule_file(&child)?);
                } else {
                    warnings.push(unsupported_extension_warning(&child));
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

pub fn normalize_rule(value: Value) -> Result<ExecutableRule> {
    let object = value.as_object().ok_or_else(|| {
        RuleModelError::InvalidRuleFormat("rule root must be a JSON object".to_owned())
    })?;

    if object.contains_key("Core") && object.contains_key("Check") && object.contains_key("Outcome")
    {
        normalize_cdisc_metadata_rule(value)
    } else if object.contains_key("core_id")
        && object.contains_key("conditions")
        && object.contains_key("actions")
    {
        serde_json::from_value(value)
            .map_err(|source| RuleModelError::InvalidRuleFormat(source.to_string()))
    } else {
        Err(RuleModelError::InvalidRuleFormat(
            "expected CDISC metadata rule or executable rule".to_owned(),
        ))
    }
}

pub fn normalize_condition_value(value: &Value) -> Result<ConditionGroup> {
    normalize_condition(value)
}

fn normalize_cdisc_metadata_rule(value: Value) -> Result<ExecutableRule> {
    let object = value.as_object().ok_or_else(|| {
        RuleModelError::InvalidRuleFormat("rule root must be a JSON object".to_owned())
    })?;

    let core = object
        .get("Core")
        .and_then(Value::as_object)
        .ok_or(RuleModelError::MissingField("Core"))?;
    let core_id = string_field(core, "Id").ok_or(RuleModelError::MissingField("Core.Id"))?;
    let status = string_field(core, "Status").map(RuleStatus::from_name);

    let authorities = array_field(object, "Authorities").unwrap_or_default();
    let standards = extract_standards(&authorities);

    let scope = object.get("Scope").and_then(Value::as_object);
    let conditions = normalize_condition(
        object
            .get("Check")
            .ok_or(RuleModelError::MissingField("Check"))?,
    )?;
    let outcome = object
        .get("Outcome")
        .and_then(Value::as_object)
        .ok_or(RuleModelError::MissingField("Outcome"))?;
    let outcome_message =
        string_field(outcome, "Message").ok_or(RuleModelError::MissingField("Outcome.Message"))?;
    let actions = vec![ActionSpec {
        name: "generate_dataset_error_objects".to_owned(),
        params: serde_json::json!({
            "message": outcome_message
        }),
    }];

    Ok(ExecutableRule {
        core_id,
        author: string_value(object.get("Author")),
        sensitivity: string_value(object.get("Sensitivity")).map(Sensitivity::from_name),
        executability: string_value(object.get("Executability")),
        description: string_value(object.get("Description")),
        authorities,
        standards,
        classes: scope.and_then(|scope| scope.get("Classes")).cloned(),
        domains: scope.and_then(|scope| scope.get("Domains")).cloned(),
        datasets: object
            .get("Match Datasets")
            .map(match_datasets_from_value)
            .transpose()?,
        entities: scope.and_then(|scope| scope.get("Entities")).cloned(),
        rule_type: string_value(object.get("Rule Type"))
            .map(RuleType::from_name)
            .unwrap_or_else(|| RuleType::Unsupported(String::new())),
        conditions,
        actions,
        operations: object
            .get("Operations")
            .map(operation_specs_from_value)
            .transpose()?
            .unwrap_or_default(),
        output_variables: string_vec_field(outcome, "Output Variables"),
        grouping_variables: string_vec_field(outcome, "Grouping Variables"),
        use_case: scope.and_then(|scope| string_field(scope, "Use Case")),
        status,
        raw: Some(value),
    })
}

fn normalize_condition(value: &Value) -> Result<ConditionGroup> {
    if let Some(expression) = value.as_str() {
        return normalize_jsonata_expression(expression);
    }

    let object = value.as_object().ok_or_else(|| {
        RuleModelError::InvalidRuleFormat("condition must be an object".to_owned())
    })?;

    if let Some(all) = object.get("all") {
        return Ok(ConditionGroup::All(normalize_condition_list(
            all,
            "Check.all",
        )?));
    }

    if let Some(any) = object.get("any") {
        return Ok(ConditionGroup::Any(normalize_condition_list(
            any,
            "Check.any",
        )?));
    }

    if let Some(not) = object.get("not") {
        return Ok(ConditionGroup::Not(Box::new(normalize_not_condition(not)?)));
    }

    let operator_name =
        string_field(object, "operator").ok_or(RuleModelError::MissingField("Check.operator"))?;
    let mut extra = BTreeMap::new();
    for (key, value) in object {
        if !matches!(
            key.as_str(),
            "name" | "target" | "operator" | "value" | "value_is_literal"
        ) {
            extra.insert(key.clone(), value.clone());
        }
    }
    let operator = Operator::from_name(operator_name);

    Ok(ConditionGroup::Leaf(Condition {
        target: string_field(object, "name").or_else(|| string_field(object, "target")),
        operator: operator.clone(),
        comparator: object
            .get("value")
            .cloned()
            .map(|value| value_expr_from_condition_value(value, object, &operator))
            .unwrap_or(ValueExpr::Null),
        options: OperatorOptions { extra },
    }))
}

fn value_expr_from_condition_value(
    value: Value,
    object: &Map<String, Value>,
    operator: &Operator,
) -> ValueExpr {
    if object
        .get("value_is_literal")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ValueExpr::Literal(value);
    }

    if let Value::String(column) = &value {
        if column.trim_start().starts_with('$') {
            return ValueExpr::ColumnRef(clean_jsonata_identifier(column));
        }
    }

    if is_column_ref_operator(operator) {
        if let Value::String(column) = &value {
            return ValueExpr::ColumnRef(column.clone());
        }
    }

    value_expr_from_value(value)
}

fn is_column_ref_operator(operator: &Operator) -> bool {
    matches!(
        operator,
        Operator::EqualTo
            | Operator::NotEqualTo
            | Operator::EqualToCaseInsensitive
            | Operator::NotEqualToCaseInsensitive
            | Operator::LessThan
            | Operator::LessThanOrEqualTo
            | Operator::GreaterThan
            | Operator::GreaterThanOrEqualTo
            | Operator::IsNotUniqueRelationship
    )
}

fn normalize_jsonata_expression(expression: &str) -> Result<ConditionGroup> {
    let expression = expression.trim();
    if is_complex_jsonata_expression(expression) {
        return Ok(unsupported_jsonata_leaf(expression));
    }

    let resolved = resolve_jsonata_bindings(expression);
    let expression = trim_wrapping_parens(resolved.trim());
    if expression.is_empty() {
        return Err(RuleModelError::InvalidRuleFormat(
            "JSONATA expression must not be empty".to_owned(),
        ));
    }

    for token in [" or ", "||"] {
        if let Some(parts) = split_top_level_expression(expression, token) {
            return parts
                .into_iter()
                .map(normalize_jsonata_expression)
                .collect::<Result<Vec<_>>>()
                .map(ConditionGroup::Any);
        }
    }

    for token in [" and ", "&&"] {
        if let Some(parts) = split_top_level_expression(expression, token) {
            return parts
                .into_iter()
                .map(normalize_jsonata_expression)
                .collect::<Result<Vec<_>>>()
                .map(ConditionGroup::All);
        }
    }

    normalize_jsonata_leaf(expression)
}

fn is_complex_jsonata_expression(expression: &str) -> bool {
    expression.contains('\n')
        || expression.contains('{')
        || expression.contains('}')
        || expression.contains('@')
        || expression.contains("~>")
}

fn normalize_jsonata_leaf(expression: &str) -> Result<ConditionGroup> {
    let expression = trim_wrapping_parens(expression.trim());
    if let Some(rest) = expression.strip_prefix('!') {
        if !rest.trim_start().starts_with('=') {
            return normalize_jsonata_expression(rest)
                .map(|group| ConditionGroup::Not(Box::new(group)));
        }
    }

    if let Some(argument) = jsonata_function_argument(expression, &["$not", "not"]) {
        return normalize_jsonata_expression(argument)
            .map(|group| ConditionGroup::Not(Box::new(group)));
    }

    if let Some(args) = jsonata_function_arguments(expression, &["$contains", "contains"]) {
        if args.len() == 2 {
            if let Ok(Value::Array(values)) = jsonata_value(args[0].trim()) {
                let (target, case_insensitive) = jsonata_target(args[1]);
                return Ok(jsonata_leaf(
                    target,
                    if case_insensitive {
                        Operator::IsContainedByCaseInsensitive
                    } else {
                        Operator::IsContainedBy
                    },
                    ValueExpr::List(values),
                ));
            }
            let (target, case_insensitive) = jsonata_target(args[0]);
            return Ok(jsonata_leaf(
                target,
                if case_insensitive {
                    Operator::ContainsCaseInsensitive
                } else {
                    Operator::Contains
                },
                jsonata_comparator(args[1])?,
            ));
        }
    }

    if let Some(args) = jsonata_function_arguments(expression, &["$match", "match"]) {
        if args.len() >= 2 {
            let (target, case_insensitive) = jsonata_target(args[0]);
            let comparator = jsonata_comparator(args[1])?;
            return Ok(jsonata_leaf(
                target,
                Operator::MatchesRegex,
                if case_insensitive {
                    case_insensitive_regex_comparator(comparator)
                } else {
                    comparator
                },
            ));
        }
    }

    if let Some(argument) = jsonata_function_argument(expression, &["$exists", "exists"]) {
        return Ok(ConditionGroup::Leaf(Condition {
            target: Some(clean_jsonata_identifier(argument)),
            operator: Operator::Exists,
            comparator: ValueExpr::Null,
            options: OperatorOptions::default(),
        }));
    }

    if let Some(argument) = jsonata_function_argument(expression, &["$boolean", "boolean"]) {
        return normalize_jsonata_expression(argument);
    }

    if let Some(index) = find_top_level_operator(expression, " not in ") {
        let (target, case_insensitive) = jsonata_target(&expression[..index]);
        let comparator = jsonata_list_comparator(&expression[index + " not in ".len()..])?;
        return Ok(jsonata_leaf(
            target,
            if case_insensitive {
                Operator::IsNotContainedByCaseInsensitive
            } else {
                Operator::IsNotContainedBy
            },
            comparator,
        ));
    }

    if let Some(index) = find_top_level_operator(expression, " in ") {
        let (target, case_insensitive) = jsonata_target(&expression[..index]);
        let comparator = jsonata_list_comparator(&expression[index + " in ".len()..])?;
        return Ok(jsonata_leaf(
            target,
            if case_insensitive {
                Operator::IsContainedByCaseInsensitive
            } else {
                Operator::IsContainedBy
            },
            comparator,
        ));
    }

    for (token, operator) in [
        (">=", Operator::GreaterThanOrEqualTo),
        ("<=", Operator::LessThanOrEqualTo),
        ("!=", Operator::NotEqualTo),
        ("=", Operator::EqualTo),
        (">", Operator::GreaterThan),
        ("<", Operator::LessThan),
    ] {
        if let Some(index) = find_top_level_operator(expression, token) {
            let left = &expression[..index];
            let comparator = jsonata_comparator(&expression[index + token.len()..])?;
            return jsonata_comparison_leaf(left, operator, comparator);
        }
    }

    if is_jsonata_identifier(expression) {
        return Ok(ConditionGroup::Leaf(Condition {
            target: Some(clean_jsonata_identifier(expression)),
            operator: Operator::Exists,
            comparator: ValueExpr::Null,
            options: OperatorOptions::default(),
        }));
    }

    Ok(unsupported_jsonata_leaf(expression))
}

fn unsupported_jsonata_leaf(expression: &str) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: None,
        operator: Operator::Unsupported("unsupported_jsonata".to_owned()),
        comparator: ValueExpr::Literal(Value::String(expression.to_owned())),
        options: OperatorOptions::default(),
    })
}

fn jsonata_leaf(target: String, operator: Operator, comparator: ValueExpr) -> ConditionGroup {
    ConditionGroup::Leaf(Condition {
        target: Some(target),
        operator,
        comparator,
        options: OperatorOptions::default(),
    })
}

fn jsonata_comparison_leaf(
    target: &str,
    operator: Operator,
    comparator: ValueExpr,
) -> Result<ConditionGroup> {
    if let Some(argument) = jsonata_function_argument(target, &["$exists", "exists"]) {
        if let Some(value) = bool_comparator(&comparator) {
            let operator = match (operator, value) {
                (Operator::EqualTo, true) | (Operator::NotEqualTo, false) => Operator::Exists,
                (Operator::EqualTo, false) | (Operator::NotEqualTo, true) => Operator::NotExists,
                _ => {
                    return Err(RuleModelError::InvalidRuleFormat(
                        "$exists() can only be compared with equality".to_owned(),
                    ))
                }
            };
            return Ok(jsonata_leaf(
                clean_jsonata_identifier(argument),
                operator,
                ValueExpr::Null,
            ));
        }
    }

    if let Some(args) = jsonata_function_arguments(target, &["$substring", "substring"]) {
        return substring_jsonata_condition(&args, &operator, &comparator);
    }

    if let Some(argument) = jsonata_function_argument(target, &["$uppercase", "uppercase"])
        .or_else(|| jsonata_function_argument(target, &["$lowercase", "lowercase"]))
    {
        return Ok(jsonata_leaf(
            clean_jsonata_identifier(argument),
            case_insensitive_jsonata_operator(operator),
            comparator,
        ));
    }

    if let Some(argument) = jsonata_function_argument(target, &["$length", "length"]) {
        return length_jsonata_condition(argument, &operator, &comparator);
    }

    Ok(jsonata_leaf(
        clean_jsonata_identifier(target),
        operator,
        comparator,
    ))
}

fn substring_jsonata_condition(
    args: &[&str],
    operator: &Operator,
    comparator: &ValueExpr,
) -> Result<ConditionGroup> {
    if !matches!(operator, Operator::EqualTo | Operator::NotEqualTo) {
        return Err(RuleModelError::InvalidRuleFormat(
            "$substring() currently supports equality comparisons".to_owned(),
        ));
    }
    if args.len() < 2 {
        return Err(RuleModelError::InvalidRuleFormat(
            "$substring() requires a target and start index".to_owned(),
        ));
    }
    let Some(start) = jsonata_value(args[1])?.as_u64() else {
        return Err(RuleModelError::InvalidRuleFormat(
            "$substring() start index must be numeric".to_owned(),
        ));
    };
    let literal = match comparator {
        ValueExpr::Literal(Value::String(value)) => value.clone(),
        _ => {
            return Err(RuleModelError::InvalidRuleFormat(
                "$substring() comparator must be a string".to_owned(),
            ))
        }
    };
    if let Some(length) = args.get(2).map(|value| jsonata_value(value)).transpose()? {
        if let Some(length) = length.as_u64() {
            if length as usize != literal.chars().count() {
                return Err(RuleModelError::InvalidRuleFormat(
                    "$substring() length must match the compared string length".to_owned(),
                ));
            }
        }
    }
    let escaped = regex_escape(&literal);
    let pattern = format!(r#"(?s)^.{{{start}}}{escaped}"#);
    Ok(jsonata_leaf(
        clean_jsonata_identifier(args[0]),
        if matches!(operator, Operator::EqualTo) {
            Operator::MatchesRegex
        } else {
            Operator::DoesNotMatchRegex
        },
        ValueExpr::Literal(Value::String(pattern)),
    ))
}

fn case_insensitive_jsonata_operator(operator: Operator) -> Operator {
    match operator {
        Operator::EqualTo => Operator::EqualToCaseInsensitive,
        Operator::NotEqualTo => Operator::NotEqualToCaseInsensitive,
        other => other,
    }
}

fn length_jsonata_condition(
    target: &str,
    operator: &Operator,
    comparator: &ValueExpr,
) -> Result<ConditionGroup> {
    let Some(value) = numeric_comparator(comparator) else {
        return Err(RuleModelError::InvalidRuleFormat(
            "$length() comparator must be numeric".to_owned(),
        ));
    };
    let target = clean_jsonata_identifier(target);

    let operator = match (operator, value) {
        (Operator::EqualTo | Operator::LessThanOrEqualTo, 0.0) => Operator::IsEmpty,
        (Operator::LessThan, 1.0) => Operator::IsEmpty,
        (Operator::NotEqualTo | Operator::GreaterThan, 0.0) => Operator::IsNotEmpty,
        (Operator::GreaterThanOrEqualTo, 1.0) => Operator::IsNotEmpty,
        _ => return length_regex_condition(target, operator, value),
    };

    Ok(jsonata_leaf(target, operator, ValueExpr::Null))
}

fn length_regex_condition(
    target: String,
    operator: &Operator,
    value: f64,
) -> Result<ConditionGroup> {
    if value.fract() != 0.0 || value < 0.0 {
        return Err(RuleModelError::InvalidRuleFormat(
            "$length() comparator must be a non-negative integer".to_owned(),
        ));
    }
    let value = value as usize;
    let (operator, pattern) = match operator {
        Operator::EqualTo => (Operator::MatchesRegex, format!(r#"(?s)^.{{{value}}}$"#)),
        Operator::NotEqualTo => (
            Operator::DoesNotMatchRegex,
            format!(r#"(?s)^.{{{value}}}$"#),
        ),
        Operator::GreaterThan => (
            Operator::MatchesRegex,
            format!(r#"(?s)^.{{{},}}$"#, value + 1),
        ),
        Operator::GreaterThanOrEqualTo => {
            (Operator::MatchesRegex, format!(r#"(?s)^.{{{value},}}$"#))
        }
        Operator::LessThan => {
            if value == 0 {
                (Operator::MatchesRegex, r#"(?!)"#.to_owned())
            } else {
                (
                    Operator::MatchesRegex,
                    format!(r#"(?s)^.{{0,{}}}$"#, value - 1),
                )
            }
        }
        Operator::LessThanOrEqualTo => (Operator::MatchesRegex, format!(r#"(?s)^.{{0,{value}}}$"#)),
        _ => {
            return Err(RuleModelError::InvalidRuleFormat(
                "$length() comparator uses an unsupported operator".to_owned(),
            ))
        }
    };
    Ok(jsonata_leaf(
        target,
        operator,
        ValueExpr::Literal(Value::String(pattern)),
    ))
}

fn numeric_comparator(comparator: &ValueExpr) -> Option<f64> {
    match comparator {
        ValueExpr::Literal(Value::Number(value)) => value.as_f64(),
        _ => None,
    }
}

fn bool_comparator(comparator: &ValueExpr) -> Option<bool> {
    match comparator {
        ValueExpr::Literal(Value::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn jsonata_function_argument<'a>(expression: &'a str, names: &[&str]) -> Option<&'a str> {
    let args = jsonata_function_arguments(expression, names)?;
    (args.len() == 1).then_some(args[0])
}

fn jsonata_function_arguments<'a>(expression: &'a str, names: &[&str]) -> Option<Vec<&'a str>> {
    let expression = expression.trim();
    let open = expression.find('(')?;
    if !names
        .iter()
        .any(|name| jsonata_function_names_equal(&expression[..open], name))
        || !expression.ends_with(')')
        || !outer_parens_enclose_expression(&expression[open..])
    {
        return None;
    }
    let argument = &expression[open + 1..expression.len() - 1];
    Some(split_top_level_commas(argument))
}

fn jsonata_function_names_equal(left: &str, right: &str) -> bool {
    left.trim()
        .trim_start_matches('$')
        .eq_ignore_ascii_case(right.trim().trim_start_matches('$'))
}

fn jsonata_list_comparator(value: &str) -> Result<ValueExpr> {
    match jsonata_value(value.trim())? {
        Value::Array(values) => Ok(ValueExpr::List(values)),
        value => Ok(ValueExpr::List(vec![value])),
    }
}

fn jsonata_comparator(value: &str) -> Result<ValueExpr> {
    let value = value.trim();
    if let Some(value) = jsonata_literal_function(value)? {
        Ok(ValueExpr::Literal(value))
    } else if is_jsonata_identifier(value) {
        Ok(ValueExpr::ColumnRef(clean_jsonata_identifier(value)))
    } else {
        Ok(value_expr_from_value(jsonata_value(value)?))
    }
}

fn jsonata_value(value: &str) -> Result<Value> {
    let value = value.trim();
    if value.starts_with('[') && value.ends_with(']') {
        let inner = &value[1..value.len() - 1];
        return split_top_level_commas(inner)
            .into_iter()
            .map(jsonata_value)
            .collect::<Result<Vec<_>>>()
            .map(Value::Array);
    }

    if value.starts_with('"') {
        return serde_json::from_str(value)
            .map_err(|source| RuleModelError::InvalidRuleFormat(source.to_string()));
    }

    if let Some(pattern) = jsonata_regex_literal(value) {
        return Ok(Value::String(pattern));
    }

    if value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2 {
        return Ok(Value::String(
            value[1..value.len() - 1]
                .replace("\\'", "'")
                .replace("\\\\", "\\"),
        ));
    }

    match value {
        "true" => return Ok(Value::Bool(true)),
        "false" => return Ok(Value::Bool(false)),
        "null" => return Ok(Value::Null),
        _ => {}
    }

    match serde_json::from_str(value) {
        Ok(value) => Ok(value),
        Err(_) => Ok(Value::String(clean_jsonata_identifier(value))),
    }
}

fn jsonata_literal_function(value: &str) -> Result<Option<Value>> {
    for (names, transform) in [
        (
            &["$uppercase", "uppercase"][..],
            JsonataLiteralTransform::Uppercase,
        ),
        (
            &["$lowercase", "lowercase"][..],
            JsonataLiteralTransform::Lowercase,
        ),
        (&["$trim", "trim"][..], JsonataLiteralTransform::Trim),
        (&["$string", "string"][..], JsonataLiteralTransform::String),
        (&["$number", "number"][..], JsonataLiteralTransform::Number),
    ] {
        if let Some(argument) = jsonata_function_argument(value, names) {
            let argument = jsonata_value(argument)?;
            return Ok(Some(apply_jsonata_literal_transform(argument, transform)));
        }
    }
    Ok(None)
}

#[derive(Debug, Clone, Copy)]
enum JsonataLiteralTransform {
    Uppercase,
    Lowercase,
    Trim,
    String,
    Number,
}

fn apply_jsonata_literal_transform(value: Value, transform: JsonataLiteralTransform) -> Value {
    match transform {
        JsonataLiteralTransform::Uppercase => {
            Value::String(json_value_string(value).to_ascii_uppercase())
        }
        JsonataLiteralTransform::Lowercase => {
            Value::String(json_value_string(value).to_ascii_lowercase())
        }
        JsonataLiteralTransform::Trim => Value::String(json_value_string(value).trim().to_owned()),
        JsonataLiteralTransform::String => Value::String(json_value_string(value)),
        JsonataLiteralTransform::Number => json_value_string(value)
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
    }
}

fn json_value_string(value: Value) -> String {
    match value {
        Value::String(value) => value,
        other => other.to_string(),
    }
}

fn jsonata_regex_literal(value: &str) -> Option<String> {
    if !value.starts_with('/') || value.len() < 2 {
        return None;
    }
    let mut escaped = false;
    for (index, ch) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '/' {
            let pattern = &value[1..index];
            let flags = &value[index + 1..];
            let prefix = if flags.contains('i') { "(?i)" } else { "" };
            return Some(format!("{prefix}{}", pattern.replace("\\/", "/")));
        }
    }
    None
}

fn jsonata_target(value: &str) -> (String, bool) {
    if let Some(argument) = jsonata_function_argument(value, &["$uppercase", "uppercase"])
        .or_else(|| jsonata_function_argument(value, &["$lowercase", "lowercase"]))
    {
        return (clean_jsonata_identifier(argument), true);
    }
    for names in [
        &["$number", "number"][..],
        &["$string", "string"][..],
        &["$trim", "trim"][..],
    ] {
        if let Some(argument) = jsonata_function_argument(value, names) {
            return (clean_jsonata_identifier(argument), false);
        }
    }
    (clean_jsonata_identifier(value), false)
}

fn case_insensitive_regex_comparator(comparator: ValueExpr) -> ValueExpr {
    match comparator {
        ValueExpr::Literal(Value::String(pattern)) if !pattern.starts_with("(?i)") => {
            ValueExpr::Literal(Value::String(format!("(?i){pattern}")))
        }
        other => other,
    }
}

fn clean_jsonata_identifier(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('$')
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
        .to_owned()
}

fn regex_escape(value: &str) -> String {
    let mut escaped = String::new();
    for ch in value.chars() {
        if matches!(
            ch,
            '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn is_jsonata_identifier(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty()
        || value.starts_with('"')
        || value.starts_with('\'')
        || value.starts_with('[')
        || matches!(value, "true" | "false" | "null")
        || serde_json::from_str::<Value>(value).is_ok()
    {
        return false;
    }

    value
        .trim_start_matches('$')
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '`'))
}

fn resolve_jsonata_bindings(expression: &str) -> String {
    let parts = split_top_level_statements(expression);
    if parts.len() <= 1 {
        return expression.to_owned();
    }

    let mut bindings = BTreeMap::new();
    for statement in &parts[..parts.len() - 1] {
        if let Some(index) = find_top_level_operator(statement, ":=") {
            let name = clean_jsonata_identifier(&statement[..index]);
            let value = trim_wrapping_parens(statement[index + 2..].trim()).to_owned();
            if !name.is_empty() && !value.is_empty() {
                bindings.insert(name, value);
            }
        }
    }

    let mut resolved = parts.last().copied().unwrap_or_default().to_owned();
    for (name, value) in bindings {
        resolved = resolved.replace(&format!("${name}"), &value);
    }
    resolved
}

fn split_top_level_statements(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ';' if depth <= 1 => {
                let part = expression[start..byte_index]
                    .trim()
                    .trim_start_matches('(')
                    .trim();
                if !part.is_empty() {
                    parts.push(part);
                }
                start = byte_index + 1;
            }
            _ => {}
        }
    }

    let mut part = expression[start..].trim();
    if expression.trim().starts_with('(') && part.ends_with(')') {
        part = part[..part.len() - 1].trim();
    }
    if !part.is_empty() {
        parts.push(part);
    }
    parts
}

fn split_top_level_expression<'a>(expression: &'a str, token: &str) -> Option<Vec<&'a str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let chars = expression.char_indices().collect::<Vec<_>>();
    let mut index = 0;

    while index < chars.len() {
        let (byte_index, ch) = chars[index];
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            index += 1;
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }

        if depth == 0 && expression[byte_index..].len() >= token.len() {
            let candidate = &expression[byte_index..byte_index + token.len()];
            if candidate.eq_ignore_ascii_case(token) {
                parts.push(expression[start..byte_index].trim());
                start = byte_index + token.len();
                while index + 1 < chars.len() && chars[index + 1].0 < start {
                    index += 1;
                }
            }
        }
        index += 1;
    }

    if parts.is_empty() {
        None
    } else {
        parts.push(expression[start..].trim());
        Some(parts)
    }
}

fn split_top_level_commas(expression: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(expression[start..byte_index].trim());
                start = byte_index + 1;
            }
            _ => {}
        }
    }
    if !expression[start..].trim().is_empty() {
        parts.push(expression[start..].trim());
    }
    parts
}

fn find_top_level_operator(expression: &str, token: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }

        if depth == 0 && expression[byte_index..].len() >= token.len() {
            let candidate = &expression[byte_index..byte_index + token.len()];
            if candidate.eq_ignore_ascii_case(token) {
                return Some(byte_index);
            }
        }
    }
    None
}

fn trim_wrapping_parens(mut expression: &str) -> &str {
    loop {
        let trimmed = expression.trim();
        if !(trimmed.starts_with('(') && trimmed.ends_with(')')) {
            return trimmed;
        }
        let inner = &trimmed[1..trimmed.len() - 1];
        if outer_parens_enclose_expression(trimmed) {
            expression = inner;
        } else {
            return trimmed;
        }
    }
}

fn outer_parens_enclose_expression(expression: &str) -> bool {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let last = expression.len() - 1;

    for (byte_index, ch) in expression.char_indices() {
        if let Some(quote_char) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote_char {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && byte_index != last {
                    return false;
                }
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && byte_index != last {
                    return false;
                }
            }
            _ => {}
        }
    }

    depth == 0
}

fn normalize_condition_list(value: &Value, field: &'static str) -> Result<Vec<ConditionGroup>> {
    let items = value
        .as_array()
        .ok_or(RuleModelError::MissingField(field))?;
    items.iter().map(normalize_condition).collect()
}

fn normalize_not_condition(value: &Value) -> Result<ConditionGroup> {
    if let Some(items) = value.as_array() {
        if items.len() == 1 {
            normalize_condition(&items[0])
        } else {
            Ok(ConditionGroup::All(
                items
                    .iter()
                    .map(normalize_condition)
                    .collect::<Result<Vec<_>>>()?,
            ))
        }
    } else {
        normalize_condition(value)
    }
}

fn deserialize_condition_group_value(value: Value) -> std::result::Result<ConditionGroup, String> {
    if let Some(object) = value.as_object() {
        if let Some(all) = object.get("all") {
            let values = all
                .as_array()
                .ok_or_else(|| "conditions.all must be an array".to_owned())?;
            return values
                .iter()
                .cloned()
                .map(deserialize_condition_group_value)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(ConditionGroup::All);
        }

        if let Some(any) = object.get("any") {
            let values = any
                .as_array()
                .ok_or_else(|| "conditions.any must be an array".to_owned())?;
            return values
                .iter()
                .cloned()
                .map(deserialize_condition_group_value)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map(ConditionGroup::Any);
        }

        if let Some(not) = object.get("not") {
            return deserialize_condition_group_value(not.clone())
                .map(Box::new)
                .map(ConditionGroup::Not);
        }

        if let Some(leaf) = object.get("leaf") {
            return serde_json::from_value(leaf.clone())
                .map(ConditionGroup::Leaf)
                .map_err(|source| format!("conditions.leaf is not a valid condition: {source}"));
        }

        if object.contains_key("operator") {
            return serde_json::from_value(Value::Object(object.clone()))
                .map(ConditionGroup::Leaf)
                .map_err(|source| format!("condition is not valid: {source}"));
        }
    }

    Err("condition group must contain all, any, not, or leaf".to_owned())
}

fn value_expr_from_value(value: Value) -> ValueExpr {
    match value {
        Value::Null => ValueExpr::Null,
        Value::Array(values) => ValueExpr::List(values),
        Value::Object(object) => {
            if let Some(value) = object.get("literal") {
                ValueExpr::Literal(value.clone())
            } else if let Some(Value::String(column)) = object.get("column_ref") {
                ValueExpr::ColumnRef(column.clone())
            } else if let Some(Value::String(column)) = object.get("column") {
                ValueExpr::ColumnRef(column.clone())
            } else if let Some(Value::Array(values)) = object.get("list") {
                ValueExpr::List(values.clone())
            } else if object.contains_key("null") {
                ValueExpr::Null
            } else {
                ValueExpr::Literal(Value::Object(object))
            }
        }
        other => ValueExpr::Literal(other),
    }
}

fn operation_specs_from_value(value: &Value) -> Result<Vec<OperationSpec>> {
    specs_from_value(value, "Operations")
}

fn match_datasets_from_value(value: &Value) -> Result<Vec<MatchDataset>> {
    specs_from_value(value, "Match Datasets")
}

fn specs_from_value<T>(value: &Value, field: &'static str) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let items = value
        .as_array()
        .ok_or(RuleModelError::MissingField(field))?;
    items
        .iter()
        .cloned()
        .map(|item| {
            serde_json::from_value(item)
                .map_err(|source| RuleModelError::InvalidRuleFormat(source.to_string()))
        })
        .collect()
}

fn extract_standards(authorities: &[Value]) -> Vec<StandardRef> {
    authorities
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|authority| {
            authority
                .get("Standards")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|standard| {
            let object = standard.as_object()?;
            let mut extra = BTreeMap::new();
            for (key, value) in object {
                if !matches!(key.as_str(), "Name" | "Version" | "name" | "version") {
                    extra.insert(key.clone(), value.clone());
                }
            }

            Some(StandardRef {
                name: string_field(object, "Name").or_else(|| string_field(object, "name")),
                version: string_field(object, "Version")
                    .or_else(|| string_field(object, "version")),
                extra,
            })
        })
        .collect()
}

fn string_vec_field(object: &Map<String, Value>, field: &str) -> Vec<String> {
    object
        .get(field)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| string_value(Some(value)))
        .collect()
}

fn array_field(object: &Map<String, Value>, field: &str) -> Option<Vec<Value>> {
    object.get(field)?.as_array().cloned()
}

fn string_field(object: &Map<String, Value>, field: &str) -> Option<String> {
    string_value(object.get(field))
}

fn string_value(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

pub fn normalize_key(key: &str) -> String {
    normalize_name(key)
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .split('_')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("_")
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

#[cfg(test)]
mod tests {
    use std::fs;

    use pretty_assertions::assert_eq;
    use serde_json::json;
    use tempfile::tempdir;

    use super::*;

    fn sample_metadata_rule() -> Value {
        json!({
            "Core": {
                "Id": "CORE-TEST-0001",
                "Status": "Published"
            },
            "Authorities": [
                {
                    "Standards": [
                        {
                            "Name": "SDTMIG",
                            "Version": "3.4"
                        }
                    ]
                }
            ],
            "Scope": {
                "Domains": {},
                "Classes": {},
                "Entities": {},
                "Use Case": "Validation"
            },
            "Sensitivity": "Record",
            "Rule Type": "Record Data",
            "Check": {
                "all": [
                    {
                        "name": "DOMAIN",
                        "operator": "equal_to",
                        "value": "AE"
                    }
                ]
            },
            "Outcome": {
                "Message": "DOMAIN must be AE"
            }
        })
    }

    #[test]
    fn load_json_rule() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("CORE-TEST-0001.json");
        fs::write(&path, sample_metadata_rule().to_string()).expect("write rule");

        let rule = load_rule_file(path).expect("load JSON rule");

        assert_eq!(rule.core_id, "CORE-TEST-0001");
    }

    #[test]
    fn load_yaml_rule() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("CORE-TEST-0002.yaml");
        fs::write(
            &path,
            r#"
Core:
  Id: CORE-TEST-0002
  Status: Published
Scope:
  Domains: {}
  Classes: {}
Sensitivity: Record
Rule Type: Record Data
Check:
  all:
    - name: DOMAIN
      operator: equal_to
      value: AE
Outcome:
  Message: DOMAIN must be AE
"#,
        )
        .expect("write rule");

        let rule = load_rule_file(path).expect("load YAML rule");

        assert_eq!(rule.core_id, "CORE-TEST-0002");
    }

    #[test]
    fn normalize_core_id_to_core_id() {
        let rule = normalize_rule(sample_metadata_rule()).expect("normalize rule");

        assert_eq!(rule.core_id, "CORE-TEST-0001");
    }

    #[test]
    fn normalize_rule_type_to_rule_type() {
        let rule = normalize_rule(sample_metadata_rule()).expect("normalize rule");

        assert_eq!(rule.rule_type, RuleType::RecordData);
    }

    #[test]
    fn normalize_outcome_variables() {
        let mut value = sample_metadata_rule();
        value["Outcome"]["Output Variables"] = json!(["AETERM", "AESTDTC", "AESER"]);
        value["Outcome"]["Grouping Variables"] = json!(["USUBJID"]);

        let rule = normalize_rule(value).expect("normalize rule");

        assert_eq!(rule.output_variables, vec!["AETERM", "AESTDTC", "AESER"]);
        assert_eq!(rule.grouping_variables, vec!["USUBJID"]);
    }

    #[test]
    fn normalize_check_all_to_condition_group_all() {
        let rule = normalize_rule(sample_metadata_rule()).expect("normalize rule");

        assert!(matches!(rule.conditions, ConditionGroup::All(_)));
    }

    #[test]
    fn normalize_check_any_to_condition_group_any() {
        let mut value = sample_metadata_rule();
        value["Check"] = json!({
            "any": [
                {
                    "name": "DOMAIN",
                    "operator": "equal_to",
                    "value": "AE"
                }
            ]
        });

        let rule = normalize_rule(value).expect("normalize rule");

        assert!(matches!(rule.conditions, ConditionGroup::Any(_)));
    }

    #[test]
    fn normalize_check_not_to_condition_group_not() {
        let mut value = sample_metadata_rule();
        value["Check"] = json!({
            "not": {
                "name": "DOMAIN",
                "operator": "equal_to",
                "value": "AE"
            }
        });

        let rule = normalize_rule(value).expect("normalize rule");

        assert!(matches!(rule.conditions, ConditionGroup::Not(_)));
    }

    #[test]
    fn normalize_leaf_condition() {
        let value = json!({
            "Core": { "Id": "CORE-TEST-0001" },
            "Scope": {},
            "Rule Type": "Record Data",
            "Check": {
                "name": "DOMAIN",
                "operator": "equal_to",
                "value": "AE",
                "value_is_literal": true,
                "case_sensitive": false
            },
            "Outcome": {
                "Message": "DOMAIN must be AE"
            }
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(condition.target.as_deref(), Some("DOMAIN"));
        assert_eq!(condition.operator, Operator::EqualTo);
        assert_eq!(condition.comparator, ValueExpr::Literal(json!("AE")));
        assert_eq!(
            condition.options.extra.get("case_sensitive"),
            Some(&json!(false))
        );
    }

    #[test]
    fn normalize_string_value_without_literal_flag_as_column_ref() {
        let value = json!({
            "Core": { "Id": "CORE-TEST-0001" },
            "Scope": {},
            "Rule Type": "Record Data",
            "Check": {
                "name": "IESTRESC",
                "operator": "not_equal_to",
                "value": "IEORRES"
            },
            "Outcome": {
                "Message": "IESTRESC must equal IEORRES"
            }
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.comparator,
            ValueExpr::ColumnRef("IEORRES".to_owned())
        );
    }

    #[test]
    fn normalize_dollar_prefixed_value_as_column_ref_for_set_comparators() {
        let value = json!({
            "Core": { "Id": "CORE-TEST-0001" },
            "Scope": {},
            "Rule Type": "Record Data",
            "Check": {
                "name": "IDVAR",
                "operator": "is_not_contained_by",
                "value": "$rdomain_variables"
            },
            "Outcome": {
                "Message": "IDVAR must exist in RDOMAIN"
            }
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.comparator,
            ValueExpr::ColumnRef("rdomain_variables".to_owned())
        );
    }

    #[test]
    fn normalize_regex_string_value_as_literal_pattern() {
        let value = json!({
            "Core": { "Id": "CORE-TEST-0001" },
            "Scope": {},
            "Rule Type": "Record Data",
            "Check": {
                "name": "DOMAIN",
                "operator": "matches_regex",
                "value": "^(.){21,}$"
            },
            "Outcome": {
                "Message": "DOMAIN is too long"
            }
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.comparator,
            ValueExpr::Literal(json!("^(.){21,}$"))
        );
    }

    #[test]
    fn yaml_value_is_literal_preserves_bare_n_as_string() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("CORE-TEST-0001.yml");
        fs::write(
            &path,
            r#"Core:
  Id: CORE-TEST-0001
Scope: {}
Rule Type: Record Data
Check:
  name: IEORRES
  operator: not_equal_to
  value: N
  value_is_literal: true
Outcome:
  Message: IEORRES must be N
"#,
        )
        .expect("write rule");

        let rule = load_rule_file(&path).expect("load YAML rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(condition.comparator, ValueExpr::Literal(json!("N")));
    }

    #[test]
    fn yaml_value_list_preserves_bare_y_n_as_strings() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("CORE-TEST-0001.yml");
        fs::write(
            &path,
            r#"Core:
  Id: CORE-TEST-0001
Scope: {}
Rule Type: Record Data
Check:
  name: AESER
  operator: is_not_contained_by
  value:
    - Y
    - N
Outcome:
  Message: AESER must be Y or N
"#,
        )
        .expect("write rule");

        let rule = load_rule_file(&path).expect("load YAML rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.comparator,
            ValueExpr::List(vec![json!("Y"), json!("N")])
        );
    }

    #[test]
    fn normalize_outcome_message_to_generate_dataset_error_objects_action() {
        let rule = normalize_rule(sample_metadata_rule()).expect("normalize rule");

        assert_eq!(rule.actions.len(), 1);
        assert_eq!(rule.actions[0].name, "generate_dataset_error_objects");
        assert_eq!(rule.actions[0].params["message"], "DOMAIN must be AE");
    }

    #[test]
    fn unknown_operator_becomes_unsupported() {
        let mut value = sample_metadata_rule();
        value["Check"] = json!({
            "name": "DOMAIN",
            "operator": "mystery_operator",
            "value": "AE"
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.operator,
            Operator::Unsupported("mystery_operator".to_owned())
        );
    }

    #[test]
    fn normalize_is_not_unique_relationship_value_as_column_ref() {
        let mut value = sample_metadata_rule();
        value["Check"] = json!({
            "name": "--TPT",
            "operator": "is_not_unique_relationship",
            "value": "--TPTNUM"
        });

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(condition.operator, Operator::IsNotUniqueRelationship);
        assert_eq!(
            condition.comparator,
            ValueExpr::ColumnRef("--TPTNUM".to_owned())
        );
    }

    #[test]
    fn open_rules_regex_operator_aliases_normalize_to_regex_operators() {
        assert_eq!(
            Operator::from_name("not_matches_regex"),
            Operator::DoesNotMatchRegexFullString
        );
    }

    #[test]
    fn open_rules_length_operator_normalizes_to_length_operator() {
        assert_eq!(Operator::from_name("longer_than"), Operator::LongerThan);
    }

    #[test]
    fn open_rules_prefix_suffix_operators_normalize_to_string_operators() {
        assert_eq!(Operator::from_name("starts_with"), Operator::StartsWith);
        assert_eq!(
            Operator::from_name("suffix_matches_regex"),
            Operator::SuffixMatchesRegex
        );
        assert_eq!(
            Operator::from_name("not_suffix_matches_regex"),
            Operator::NotSuffixMatchesRegex
        );
    }

    #[test]
    fn open_rules_date_and_suffix_operator_names_normalize() {
        assert_eq!(Operator::from_name("ends_with"), Operator::EndsWith);
        assert_eq!(Operator::from_name("date_equal_to"), Operator::DateEqualTo);
        assert_eq!(
            Operator::from_name("date_less_than"),
            Operator::DateLessThan
        );
        assert_eq!(
            Operator::from_name("date_less_than_or_equal_to"),
            Operator::DateLessThanOrEqualTo
        );
        assert_eq!(
            Operator::from_name("date_greater_than"),
            Operator::DateGreaterThan
        );
        assert_eq!(
            Operator::from_name("date_greater_than_or_equal_to"),
            Operator::DateGreaterThanOrEqualTo
        );
        assert_eq!(Operator::from_name("invalid_date"), Operator::InvalidDate);
        assert_eq!(
            Operator::from_name("invalid_duration"),
            Operator::InvalidDuration
        );
        assert_eq!(
            Operator::from_name("is_complete_date"),
            Operator::IsCompleteDate
        );
        assert_eq!(
            Operator::from_name("is_incomplete_date"),
            Operator::IsIncompleteDate
        );
    }

    #[test]
    fn open_rules_order_operator_name_normalizes() {
        assert_eq!(
            Operator::from_name("target_is_not_sorted_by"),
            Operator::TargetIsNotSortedBy
        );
        assert_eq!(
            Operator::from_name("empty_within_except_last_row"),
            Operator::EmptyWithinExceptLastRow
        );
        assert_eq!(
            Operator::from_name("does_not_have_next_corresponding_record"),
            Operator::DoesNotHaveNextCorrespondingRecord
        );
        assert_eq!(
            Operator::from_name("not_present_on_multiple_rows_within"),
            Operator::NotPresentOnMultipleRowsWithin
        );
        assert_eq!(
            Operator::from_name("inconsistent_enumerated_columns"),
            Operator::InconsistentEnumeratedColumns
        );
        assert_eq!(
            Operator::from_name("is_not_unique_set"),
            Operator::IsNotUniqueSet
        );
        assert_eq!(Operator::from_name("is_unique_set"), Operator::IsUniqueSet);
        assert_eq!(
            Operator::from_name("is_not_unique_relationship"),
            Operator::IsNotUniqueRelationship
        );
        assert_eq!(
            Operator::from_name("is_inconsistent_across_dataset"),
            Operator::IsInconsistentAcrossDataset
        );
        assert_eq!(
            Operator::from_name("does_not_equal_string_part"),
            Operator::DoesNotEqualStringPart
        );
    }

    #[test]
    fn open_rules_empty_operators_normalize_to_empty_operators() {
        assert_eq!(Operator::from_name("empty"), Operator::IsEmpty);
        assert_eq!(Operator::from_name("non_empty"), Operator::IsNotEmpty);
    }

    #[test]
    fn jsonata_rule_type_becomes_jsonata() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");

        let rule = normalize_rule(value).expect("normalize rule");

        assert_eq!(rule.rule_type, RuleType::Jsonata);
    }

    #[test]
    fn normalize_jsonata_string_expression_to_condition_tree() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!("$exists(DOMAIN) and DOMAIN != 'AE'");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::All(groups) = rule.conditions else {
            panic!("expected all condition group");
        };

        assert_eq!(groups.len(), 2);
        let ConditionGroup::Leaf(exists) = &groups[0] else {
            panic!("expected exists leaf");
        };
        assert_eq!(exists.target.as_deref(), Some("DOMAIN"));
        assert_eq!(exists.operator, Operator::Exists);

        let ConditionGroup::Leaf(compare) = &groups[1] else {
            panic!("expected comparison leaf");
        };
        assert_eq!(compare.target.as_deref(), Some("DOMAIN"));
        assert_eq!(compare.operator, Operator::NotEqualTo);
        assert_eq!(compare.comparator, ValueExpr::Literal(json!("AE")));
    }

    #[test]
    fn normalize_unsupported_jsonata_expression_as_unsupported_operator() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!("$.study.versions.studyDesigns.{\"id\": id}[id != null]");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(
            condition.operator,
            Operator::Unsupported("unsupported_jsonata".to_owned())
        );
        assert_eq!(condition.target, None);
    }

    #[test]
    fn normalize_jsonata_in_expression_to_list_comparator() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!("DOMAIN not in ['AE', 'CM']");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Leaf(condition) = rule.conditions else {
            panic!("expected leaf condition");
        };

        assert_eq!(condition.target.as_deref(), Some("DOMAIN"));
        assert_eq!(condition.operator, Operator::IsNotContainedBy);
        assert_eq!(
            condition.comparator,
            ValueExpr::List(vec![json!("AE"), json!("CM")])
        );
    }

    #[test]
    fn normalize_jsonata_function_expressions() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!("$not($contains(AETERM, 'headache')) or $match(AEDECOD, '^HEAD')");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Any(groups) = rule.conditions else {
            panic!("expected any condition group");
        };

        let ConditionGroup::Not(contains_group) = &groups[0] else {
            panic!("expected not condition");
        };
        let ConditionGroup::Leaf(contains) = contains_group.as_ref() else {
            panic!("expected contains leaf");
        };
        assert_eq!(contains.target.as_deref(), Some("AETERM"));
        assert_eq!(contains.operator, Operator::Contains);
        assert_eq!(contains.comparator, ValueExpr::Literal(json!("headache")));

        let ConditionGroup::Leaf(matches) = &groups[1] else {
            panic!("expected match leaf");
        };
        assert_eq!(matches.target.as_deref(), Some("AEDECOD"));
        assert_eq!(matches.operator, Operator::MatchesRegex);
        assert_eq!(matches.comparator, ValueExpr::Literal(json!("^HEAD")));
    }

    #[test]
    fn normalize_jsonata_case_and_length_functions() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!("$uppercase(domain) = 'AE' and $length(AETERM) > 0");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::All(groups) = rule.conditions else {
            panic!("expected all condition group");
        };

        let ConditionGroup::Leaf(case_condition) = &groups[0] else {
            panic!("expected case condition");
        };
        assert_eq!(case_condition.target.as_deref(), Some("domain"));
        assert_eq!(case_condition.operator, Operator::EqualToCaseInsensitive);
        assert_eq!(case_condition.comparator, ValueExpr::Literal(json!("AE")));

        let ConditionGroup::Leaf(length_condition) = &groups[1] else {
            panic!("expected length condition");
        };
        assert_eq!(length_condition.target.as_deref(), Some("AETERM"));
        assert_eq!(length_condition.operator, Operator::IsNotEmpty);
        assert_eq!(length_condition.comparator, ValueExpr::Null);
    }

    #[test]
    fn normalize_jsonata_extended_boolean_functions_and_bindings() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] =
            json!("($domain := DOMAIN; !$exists(AETERM) || $contains($uppercase($domain), 'ae'))");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Any(groups) = rule.conditions else {
            panic!("expected any condition group");
        };

        let ConditionGroup::Not(exists_group) = &groups[0] else {
            panic!("expected not exists condition");
        };
        let ConditionGroup::Leaf(exists) = exists_group.as_ref() else {
            panic!("expected exists leaf");
        };
        assert_eq!(exists.target.as_deref(), Some("AETERM"));
        assert_eq!(exists.operator, Operator::Exists);

        let ConditionGroup::Leaf(contains) = &groups[1] else {
            panic!("expected contains leaf");
        };
        assert_eq!(contains.target.as_deref(), Some("DOMAIN"));
        assert_eq!(contains.operator, Operator::ContainsCaseInsensitive);
        assert_eq!(contains.comparator, ValueExpr::Literal(json!("ae")));
    }

    #[test]
    fn normalize_jsonata_regex_literals_exists_comparisons_and_length_ranges() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] = json!(
            "$exists(AETERM) = false or $match($lowercase(AEDECOD), /^head/i) or $length(AEDECOD) >= 4"
        );

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::Any(groups) = rule.conditions else {
            panic!("expected any condition group");
        };

        let ConditionGroup::Leaf(exists) = &groups[0] else {
            panic!("expected exists comparison leaf");
        };
        assert_eq!(exists.target.as_deref(), Some("AETERM"));
        assert_eq!(exists.operator, Operator::NotExists);

        let ConditionGroup::Leaf(matches) = &groups[1] else {
            panic!("expected regex leaf");
        };
        assert_eq!(matches.target.as_deref(), Some("AEDECOD"));
        assert_eq!(matches.operator, Operator::MatchesRegex);
        assert_eq!(matches.comparator, ValueExpr::Literal(json!("(?i)^head")));

        let ConditionGroup::Leaf(length) = &groups[2] else {
            panic!("expected length regex leaf");
        };
        assert_eq!(length.target.as_deref(), Some("AEDECOD"));
        assert_eq!(length.operator, Operator::MatchesRegex);
        assert_eq!(length.comparator, ValueExpr::Literal(json!("(?s)^.{4,}$")));
    }

    #[test]
    fn normalize_jsonata_array_contains_and_substring_comparisons() {
        let mut value = sample_metadata_rule();
        value["Rule Type"] = json!("JSONATA");
        value["Check"] =
            json!("$contains(['AE', 'CM'], DOMAIN) and $substring(USUBJID, 0, 4) = 'SUBJ'");

        let rule = normalize_rule(value).expect("normalize rule");
        let ConditionGroup::All(groups) = rule.conditions else {
            panic!("expected all condition group");
        };

        let ConditionGroup::Leaf(contains) = &groups[0] else {
            panic!("expected contains leaf");
        };
        assert_eq!(contains.target.as_deref(), Some("DOMAIN"));
        assert_eq!(contains.operator, Operator::IsContainedBy);
        assert_eq!(
            contains.comparator,
            ValueExpr::List(vec![json!("AE"), json!("CM")])
        );

        let ConditionGroup::Leaf(substring) = &groups[1] else {
            panic!("expected substring leaf");
        };
        assert_eq!(substring.target.as_deref(), Some("USUBJID"));
        assert_eq!(substring.operator, Operator::MatchesRegex);
        assert_eq!(
            substring.comparator,
            ValueExpr::Literal(json!("(?s)^.{0}SUBJ"))
        );
    }

    #[test]
    fn executable_format_passes_through_without_cdisc_metadata_normalization() {
        let value = json!({
            "core_id": "CORE-EXEC-0001",
            "rule_type": "record_data",
            "conditions": {
                "leaf": {
                    "target": "DOMAIN",
                    "operator": "equal_to",
                    "comparator": {
                        "literal": "AE"
                    }
                }
            },
            "actions": [
                {
                    "name": "generate_dataset_error_objects",
                    "params": {
                        "message": "DOMAIN must be AE"
                    }
                }
            ]
        });

        let rule = normalize_rule(value).expect("normalize rule");

        assert_eq!(rule.core_id, "CORE-EXEC-0001");
        assert_eq!(rule.rule_type, RuleType::RecordData);
        assert_eq!(rule.raw, None);
    }

    #[test]
    fn load_rules_from_directory_returns_warnings_for_unsupported_extensions() {
        let dir = tempdir().expect("tempdir");
        fs::write(
            dir.path().join("CORE-TEST-0001.json"),
            sample_metadata_rule().to_string(),
        )
        .expect("write rule");
        fs::write(dir.path().join("notes.txt"), "not a rule").expect("write notes");

        let result = load_rules_from_paths_with_warnings(&[dir.path().to_path_buf()])
            .expect("load rules from dir");

        assert_eq!(result.rules.len(), 1);
        assert_eq!(result.warnings.len(), 1);
        assert_eq!(
            result.warnings[0].kind,
            LoadWarningKind::UnsupportedExtension("txt".to_owned())
        );
    }
}
