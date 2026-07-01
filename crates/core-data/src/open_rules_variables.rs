use std::collections::{BTreeMap, BTreeSet};

use super::{
    normalize_dataset_name, row_string, CsvRecords, DataError, DatasetVariable, OpenRulesDataset,
    OpenRulesVariable, Result,
};

fn open_rules_variable_descriptor(
    row: &BTreeMap<String, String>,
    raw_label: Option<String>,
) -> Result<OpenRulesVariable> {
    let dataset =
        row_string(row, &["dataset", "Dataset", "domain", "Domain"]).ok_or_else(|| {
            DataError::InvalidDatasetPackage("_variables.csv missing dataset".to_owned())
        })?;
    let variable = row_string(
        row,
        &[
            "variable",
            "Variable",
            "name",
            "Name",
            "variable_name",
            "Variable Name",
        ],
    )
    .ok_or_else(|| {
        DataError::InvalidDatasetPackage("_variables.csv missing variable".to_owned())
    })?;
    let variable_type = row_string(row, &["type", "Type", "DataType", "datatype"]);
    let length =
        row_string(row, &["length", "Length"]).and_then(|value| value.parse::<usize>().ok());

    Ok(OpenRulesVariable {
        dataset: normalize_dataset_name(&dataset),
        variable: DatasetVariable {
            name: normalize_open_rules_variable_name(variable.trim()),
            label: raw_label.or_else(|| row_string(row, &["label", "Label"])),
            variable_type,
            length,
            extra: BTreeMap::new(),
        },
    })
}

fn normalize_open_rules_variable_name(name: &str) -> String {
    let has_lowercase = name.chars().any(|ch| ch.is_ascii_lowercase());
    let has_uppercase = name.chars().any(|ch| ch.is_ascii_uppercase());
    if has_lowercase && has_uppercase {
        name.to_owned()
    } else {
        name.to_ascii_uppercase()
    }
}

pub(super) fn open_rules_variable_descriptors(
    records: &CsvRecords,
    rows: &[BTreeMap<String, String>],
    datasets: &[OpenRulesDataset],
) -> Result<Vec<OpenRulesVariable>> {
    let dictionary_result = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            open_rules_variable_descriptor(row, open_rules_variable_raw_label(records, index))
        })
        .collect::<Result<Vec<_>>>();
    match dictionary_result {
        Ok(variables) => Ok(variables),
        Err(source) if is_horizontal_open_rules_variables_schema(records) => {
            horizontal_open_rules_variable_descriptors(records, datasets).ok_or(source)
        }
        Err(source) => Err(source),
    }
}

fn open_rules_variable_raw_label(records: &CsvRecords, row_index: usize) -> Option<String> {
    let label_index = records
        .headers
        .iter()
        .position(|header| header.trim().eq_ignore_ascii_case("label"))?;
    records
        .records
        .get(row_index)
        .and_then(|row| row.get(label_index))
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn is_horizontal_open_rules_variables_schema(records: &CsvRecords) -> bool {
    !records.headers.is_empty()
        && records.headers.iter().all(|header| {
            !matches!(
                header.trim().to_ascii_lowercase().as_str(),
                "dataset" | "variable" | "name" | "variable_name" | "variable name"
            )
        })
}

fn horizontal_open_rules_variable_descriptors(
    records: &CsvRecords,
    datasets: &[OpenRulesDataset],
) -> Option<Vec<OpenRulesVariable>> {
    let dataset = single_open_rules_dataset_name(datasets)?;
    let labels = records.records.first();
    let types = records.records.get(1);
    let lengths = records.records.get(2);
    Some(
        records
            .headers
            .iter()
            .enumerate()
            .filter_map(|(index, name)| {
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                Some(OpenRulesVariable {
                    dataset: dataset.clone(),
                    variable: DatasetVariable {
                        name: name.to_ascii_uppercase(),
                        label: labels
                            .and_then(|row| row.get(index))
                            .map(|value| value.trim())
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                        variable_type: types
                            .and_then(|row| row.get(index))
                            .map(|value| value.trim())
                            .filter(|value| !value.is_empty())
                            .map(str::to_owned),
                        length: lengths
                            .and_then(|row| row.get(index))
                            .and_then(|value| value.trim().parse::<usize>().ok()),
                        extra: BTreeMap::new(),
                    },
                })
            })
            .collect(),
    )
}

fn single_open_rules_dataset_name(datasets: &[OpenRulesDataset]) -> Option<String> {
    let mut names = datasets
        .iter()
        .map(|dataset| dataset.name.clone())
        .collect::<BTreeSet<_>>();
    (names.len() == 1).then(|| names.pop_first()).flatten()
}
