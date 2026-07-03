use std::collections::BTreeSet;

use core_data::{dataset_column_values, LoadedDataset};
use core_rule_model::ExecutableRule;
use serde_json::Value;

use crate::json_values::json_scalar_string;

pub(crate) fn filter_datasets_by_rule_scope(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Vec<LoadedDataset> {
    if rule.entities.is_some() {
        let scoped = datasets
            .iter()
            .filter(|dataset| entity_scope_allows(rule.entities.as_ref(), dataset))
            .cloned()
            .collect::<Vec<_>>();
        return deduplicate_entity_scope_datasets_by_path(scoped);
    }
    filter_datasets_by_domain_scope(rule, datasets)
}

fn deduplicate_entity_scope_datasets_by_path(datasets: Vec<LoadedDataset>) -> Vec<LoadedDataset> {
    let mut seen_paths = BTreeSet::new();
    datasets
        .into_iter()
        .filter(|dataset| {
            let Some(path) = first_dataset_path_value(dataset) else {
                return true;
            };
            seen_paths.insert(path)
        })
        .collect()
}

fn first_dataset_path_value(dataset: &LoadedDataset) -> Option<String> {
    dataset_column_values(dataset, "path")
        .ok()
        .and_then(|values| values.first().and_then(json_scalar_string))
        .filter(|path| !path.is_empty())
}

fn filter_datasets_by_domain_scope(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
) -> Vec<LoadedDataset> {
    datasets
        .iter()
        .filter(|dataset| {
            domain_scope_allows(rule.domains.as_ref(), dataset)
                && class_scope_allows(rule.classes.as_ref(), dataset)
        })
        .cloned()
        .collect()
}

fn domain_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name);
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");

    if scope_matches(&excludes, domain) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || scope_matches(&includes, domain)
}

fn entity_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let entity = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name);
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");

    if scope_matches(&excludes, entity) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || scope_matches(&includes, entity)
}

pub(crate) fn scope_values(scope: Option<&Value>, key: &str) -> Vec<String> {
    let Some(object) = scope.and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(value) = object.get(key).or_else(|| {
        object
            .iter()
            .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
            .map(|(_key, value)| value)
    }) else {
        return Vec::new();
    };

    match value {
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Value::String(value) => vec![value.clone()],
        _ => Vec::new(),
    }
}

pub(crate) fn scope_contains_all(values: &[String]) -> bool {
    values.iter().any(|value| value.eq_ignore_ascii_case("ALL"))
}

pub(crate) fn scope_matches(values: &[String], domain: &str) -> bool {
    values
        .iter()
        .any(|value| domain_scope_matches(value, domain))
}

pub(crate) fn domain_scope_matches(pattern: &str, domain: &str) -> bool {
    if pattern.eq_ignore_ascii_case(domain) {
        return true;
    }
    if let Some((prefix, suffix)) = pattern.split_once("--") {
        return domain
            .to_ascii_uppercase()
            .starts_with(&prefix.to_ascii_uppercase())
            && domain
                .to_ascii_uppercase()
                .ends_with(&suffix.to_ascii_uppercase());
    }
    false
}

fn class_scope_allows(scope: Option<&Value>, dataset: &LoadedDataset) -> bool {
    let includes = scope_values(scope, "Include");
    let excludes = scope_values(scope, "Exclude");
    let Some(class) = dataset_domain_class(dataset) else {
        return true;
    };

    if class_scope_matches(&excludes, class) {
        return false;
    }
    includes.is_empty() || scope_contains_all(&includes) || class_scope_matches(&includes, class)
}

fn dataset_domain_class(dataset: &LoadedDataset) -> Option<&'static str> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(&dataset.metadata.name);
    domain_class_name(domain)
}

pub(crate) fn domain_class_name(domain: &str) -> Option<&'static str> {
    let domain = domain.to_ascii_uppercase();
    match domain.as_str() {
        "CM" | "EC" | "EX" | "ML" | "PR" | "SU" => Some("INTERVENTIONS"),
        "AE" | "CE" | "DS" | "DV" | "MH" => Some("EVENTS"),
        "CV" | "DD" | "EG" | "FT" | "IE" | "IS" | "LB" | "MB" | "MI" | "MS" | "PC" | "PP"
        | "QS" | "RE" | "RP" | "SC" | "SS" | "TR" | "TU" | "UR" | "VS" => Some("FINDINGS"),
        "FA" | "SR" => Some("FINDINGS ABOUT"),
        "CO" | "DM" | "SE" | "SV" => Some("SPECIAL PURPOSE"),
        "TA" | "TD" | "TE" | "TI" | "TM" | "TS" | "TV" => Some("TRIAL DESIGN"),
        "RELREC" | "SUPP" | "SUPPQUAL" => Some("RELATIONSHIP"),
        _ => None,
    }
}

pub(crate) fn class_scope_matches(values: &[String], class: &str) -> bool {
    let normalized_class = normalize_scope_class(class);
    values.iter().any(|value| {
        let normalized_value = normalize_scope_class(value);
        normalized_value == normalized_class
            || (normalized_value == "FINDINGS" && normalized_class == "FINDINGSABOUT")
    })
}

fn normalize_scope_class(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !matches!(ch, ' ' | '_' | '-'))
        .collect::<String>()
        .to_ascii_uppercase()
}
