use core_data::{dataset_column_values, LoadedDataset};
use serde_json::Value;

pub(crate) fn push_unique_string(values: &mut Vec<String>, value: &str) {
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_owned());
    }
}

pub(crate) fn value_is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        _ => false,
    }
}

pub(crate) fn dataset_has_column(dataset: &LoadedDataset, name: &str) -> bool {
    dataset_column_name(dataset, name).is_some()
}

pub(crate) fn dataset_column_name(dataset: &LoadedDataset, name: &str) -> Option<String> {
    dataset
        .frame()
        .get_column_names()
        .iter()
        .find(|column| column.as_str().eq_ignore_ascii_case(name))
        .map(|column| column.as_str().to_owned())
}

pub(crate) fn dataset_metadata_name(dataset: &LoadedDataset) -> String {
    dataset
        .metadata
        .filename
        .split('.')
        .next()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(&dataset.metadata.name)
        .to_ascii_uppercase()
}

pub(crate) fn dataset_domain_value(dataset: &LoadedDataset) -> String {
    dataset_column_values(dataset, "DOMAIN")
        .ok()
        .and_then(|values| {
            values.into_iter().find_map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
        })
        .or_else(|| dataset.metadata.domain.clone())
        .unwrap_or_else(|| dataset.metadata.name.clone())
        .to_ascii_uppercase()
}
