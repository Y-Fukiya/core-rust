use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

use indexmap::IndexMap;
use serde_json::Value;

use super::{
    canonical_or_original, csv_records_to_dict_rows, extension, file_name, file_stem_str,
    load_open_rules_json_data_dir, normalize_dataset_name, normalize_metadata_name, number_value,
    parse_csv_bool, read_csv_dict_rows, read_csv_records, records_to_frame, row_string, CsvRecords,
    DataError, DatasetMetadata, DatasetSourceFormat, DatasetVariable, LoadDataResult,
    LoadDataWarning, LoadDataWarningKind, LoadedDataset, Result,
};
use crate::open_rules_variables::open_rules_variable_descriptors;

pub fn load_open_rules_data_dir(path: impl AsRef<Path>) -> Result<Vec<LoadedDataset>> {
    Ok(load_open_rules_data_dir_with_warnings(path)?.datasets)
}

pub fn load_open_rules_data_dir_with_warnings(path: impl AsRef<Path>) -> Result<LoadDataResult> {
    load_open_rules_data_dir_impl(path.as_ref())
}

fn load_open_rules_data_dir_impl(data_dir: &Path) -> Result<LoadDataResult> {
    let variables_path = data_dir.join("_variables.csv");
    if !variables_path.is_file() && !has_open_rules_dataset_csv(data_dir)? {
        return load_open_rules_json_data_dir(data_dir);
    }

    let variable_records = read_csv_records(&variables_path)?;
    let variable_rows = csv_records_to_dict_rows(&variable_records);
    let datasets_path = data_dir.join("_datasets.csv");
    let datasets = if datasets_path.is_file() {
        read_csv_dict_rows(&datasets_path)?
            .iter()
            .filter(|row| !is_blank_csv_dict_row(row))
            .map(open_rules_dataset_descriptor)
            .collect::<Result<Vec<_>>>()?
    } else {
        infer_open_rules_dataset_descriptors(data_dir, &variable_rows)?
    };
    let variables = open_rules_variable_descriptors(&variable_records, &variable_rows, &datasets)?;

    let mut variables_by_dataset = BTreeMap::<String, Vec<OpenRulesVariable>>::new();
    for variable in &variables {
        variables_by_dataset
            .entry(variable.dataset.clone())
            .or_default()
            .push(variable.clone());
    }

    let mut loaded = Vec::new();
    let mut warnings = Vec::new();
    for dataset in datasets {
        let filename_dataset = normalize_dataset_name(file_stem_str(&dataset.filename));
        let variables = variables_by_dataset
            .get(&dataset.name)
            .or_else(|| variables_by_dataset.get(&filename_dataset))
            .cloned()
            .unwrap_or_default();
        loaded.push(load_open_rules_dataset(
            data_dir,
            dataset,
            variables,
            &variables_by_dataset,
            &mut warnings,
        )?);
    }

    Ok(LoadDataResult {
        datasets: loaded,
        warnings,
    })
}

#[derive(Debug, Clone)]
pub(super) struct OpenRulesDataset {
    pub(super) name: String,
    pub(super) filename: String,
    pub(super) label: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct OpenRulesVariable {
    pub(super) dataset: String,
    pub(super) variable: DatasetVariable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenRulesVariableKind {
    Boolean,
    Character,
    Numeric,
    Unknown,
}

fn open_rules_dataset_descriptor(row: &BTreeMap<String, String>) -> Result<OpenRulesDataset> {
    let filename = row_string(
        row,
        &[
            "Filename", "filename", "File", "file", "Dataset", "dataset", "Name", "name",
        ],
    )
    .ok_or_else(|| DataError::InvalidDatasetPackage("_datasets.csv missing Filename".to_owned()))?;
    let filename = ensure_csv_filename(&filename);
    let name = row_string(
        row,
        &[
            "Dataset Name",
            "dataset name",
            "Dataset",
            "dataset",
            "Domain",
            "domain",
            "Name",
            "name",
        ],
    )
    .unwrap_or_else(|| file_stem_str(&filename).to_owned());

    Ok(OpenRulesDataset {
        name: normalize_dataset_name(&name),
        filename,
        label: row_string(row, &["Label", "label", "Description", "description"]),
    })
}

fn is_blank_csv_dict_row(row: &BTreeMap<String, String>) -> bool {
    row.values().all(|value| value.trim().is_empty())
}

fn infer_open_rules_dataset_descriptors(
    data_dir: &Path,
    variable_rows: &[BTreeMap<String, String>],
) -> Result<Vec<OpenRulesDataset>> {
    let mut datasets = variable_rows
        .iter()
        .filter_map(|row| row_string(row, &["dataset", "Dataset", "domain", "Domain"]))
        .map(|name| normalize_dataset_name(&name))
        .collect::<BTreeSet<_>>();

    if datasets.is_empty() {
        let entries = fs::read_dir(data_dir)
            .map_err(|source| DataError::Io {
                path: data_dir.to_path_buf(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| DataError::Io {
                path: data_dir.to_path_buf(),
                source,
            })?;
        for entry in entries {
            let path = entry.path();
            if !path.is_file() || extension(&path).as_deref() != Some("csv") {
                continue;
            }
            let filename = file_name(&path)?;
            if is_open_rules_auxiliary_csv(&filename) {
                continue;
            }
            datasets.insert(file_stem_str(&filename).to_ascii_uppercase());
        }
    }

    datasets
        .into_iter()
        .map(|name| {
            let filename = find_open_rules_dataset_filename(data_dir, &name)?
                .unwrap_or_else(|| ensure_csv_filename(&name.to_ascii_lowercase()));
            Ok(OpenRulesDataset {
                name,
                filename,
                label: None,
            })
        })
        .collect()
}

fn find_open_rules_dataset_filename(data_dir: &Path, dataset: &str) -> Result<Option<String>> {
    let entries = fs::read_dir(data_dir)
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?;
    for entry in entries {
        let path = entry.path();
        if !path.is_file() || extension(&path).as_deref() != Some("csv") {
            continue;
        }
        let filename = file_name(&path)?;
        if is_open_rules_auxiliary_csv(&filename) {
            continue;
        }
        if file_stem_str(&filename).eq_ignore_ascii_case(dataset) {
            return Ok(Some(filename));
        }
    }
    Ok(None)
}

fn has_open_rules_dataset_csv(data_dir: &Path) -> Result<bool> {
    let entries = fs::read_dir(data_dir)
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source| DataError::Io {
            path: data_dir.to_path_buf(),
            source,
        })?;
    for entry in entries {
        let path = entry.path();
        if !path.is_file() || extension(&path).as_deref() != Some("csv") {
            continue;
        }
        let filename = file_name(&path)?;
        if !is_open_rules_auxiliary_csv(&filename) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_open_rules_auxiliary_csv(filename: &str) -> bool {
    matches!(
        filename.to_ascii_lowercase().as_str(),
        "_datasets.csv" | "_variables.csv" | "results.csv"
    )
}

fn load_open_rules_dataset(
    data_dir: &Path,
    dataset: OpenRulesDataset,
    mut variables: Vec<OpenRulesVariable>,
    variables_by_dataset: &BTreeMap<String, Vec<OpenRulesVariable>>,
    warnings: &mut Vec<LoadDataWarning>,
) -> Result<LoadedDataset> {
    let path = resolve_open_rules_dataset_path(data_dir, &dataset.filename)?;
    let raw_rows = read_csv_records(&path)?;
    if variables.is_empty() {
        variables =
            infer_open_rules_embedded_metadata_variables(&dataset, &raw_rows, variables_by_dataset);
    }
    let declared = variables
        .iter()
        .map(|variable| {
            (
                variable.variable.name.to_ascii_uppercase(),
                variable.clone(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let label_columns = open_rules_label_column_map(&variables);
    let rows = normalize_open_rules_embedded_metadata_records(raw_rows, &declared, &label_columns);
    let mut seen_columns = BTreeSet::new();
    let csv_columns = rows
        .headers
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let name = open_rules_csv_column_name(name, &declared, &label_columns);
            (!name.is_empty() && seen_columns.insert(name.clone())).then_some((index, name))
        })
        .collect::<Vec<_>>();
    let mut records = csv_columns
        .iter()
        .map(|(_index, name)| (name.clone(), Vec::with_capacity(rows.records.len())))
        .collect::<IndexMap<_, _>>();

    for (_index, column) in &csv_columns {
        if !declared.contains_key(column) {
            warnings.push(LoadDataWarning {
                path: path.clone(),
                kind: LoadDataWarningKind::UndeclaredCsvColumn {
                    dataset: dataset.name.clone(),
                    variable: column.clone(),
                },
            });
        }
    }

    for variable in declared.keys() {
        if !csv_columns
            .iter()
            .any(|(_index, column)| column == variable)
        {
            warnings.push(LoadDataWarning {
                path: path.clone(),
                kind: LoadDataWarningKind::DeclaredVariableMissing {
                    dataset: dataset.name.clone(),
                    variable: variable.clone(),
                },
            });
        }
    }

    for (row_index, record) in rows.records.iter().enumerate() {
        for (index, column) in &csv_columns {
            let raw = record.get(*index).map_or("", String::as_str);
            let value = open_rules_cell_value(
                &path,
                &dataset.name,
                column,
                declared.get(column),
                raw,
                row_index + 1,
                warnings,
            );
            records
                .get_mut(column)
                .expect("record column initialized")
                .push(value);
        }
    }

    let frame = records_to_frame(&records).map_err(|source| DataError::Polars {
        path: path.clone(),
        source,
    })?;
    let metadata = DatasetMetadata {
        name: dataset.name.clone(),
        domain: Some(dataset.name),
        label: dataset.label,
        filename: file_name(&path)?,
        full_path: canonical_or_original(&path),
        source_format: DatasetSourceFormat::Csv,
        variables: variables
            .into_iter()
            .map(|variable| variable.variable)
            .collect(),
    };

    Ok(LoadedDataset::new(metadata, frame))
}

fn infer_open_rules_embedded_metadata_variables(
    dataset: &OpenRulesDataset,
    rows: &CsvRecords,
    variables_by_dataset: &BTreeMap<String, Vec<OpenRulesVariable>>,
) -> Vec<OpenRulesVariable> {
    if !rows.headers.iter().all(|header| header.trim().is_empty()) {
        return Vec::new();
    }
    let Some(labels) = rows.records.first() else {
        return Vec::new();
    };
    let types = rows.records.get(1);
    let lengths = rows.records.get(2);
    labels
        .iter()
        .enumerate()
        .filter_map(|(index, label)| {
            let name = infer_open_rules_embedded_metadata_variable_name(
                &dataset.name,
                label,
                variables_by_dataset,
            )?;
            Some(OpenRulesVariable {
                dataset: dataset.name.clone(),
                variable: DatasetVariable {
                    name,
                    label: Some(label.clone()),
                    variable_type: types
                        .and_then(|row| row.get(index))
                        .map(|value| value.trim().to_owned())
                        .filter(|value| !value.is_empty()),
                    length: lengths
                        .and_then(|row| row.get(index))
                        .and_then(|value| value.trim().parse::<usize>().ok()),
                    extra: BTreeMap::new(),
                },
            })
        })
        .collect()
}

fn infer_open_rules_embedded_metadata_variable_name(
    dataset: &str,
    label: &str,
    variables_by_dataset: &BTreeMap<String, Vec<OpenRulesVariable>>,
) -> Option<String> {
    let label = normalize_open_rules_header_label(label);
    if label.is_empty() {
        return None;
    }
    let dataset = dataset.to_ascii_uppercase();
    let suffixes = variables_by_dataset
        .values()
        .flatten()
        .filter(|variable| {
            variable
                .variable
                .label
                .as_deref()
                .map(normalize_open_rules_header_label)
                .as_deref()
                == Some(label.as_str())
        })
        .filter_map(|variable| {
            let name = variable.variable.name.to_ascii_uppercase();
            let source_dataset = variable.dataset.to_ascii_uppercase();
            if matches!(name.as_str(), "STUDYID" | "DOMAIN" | "USUBJID" | "POOLID") {
                return Some(name);
            }
            name.strip_prefix(&source_dataset)
                .filter(|suffix| !suffix.is_empty())
                .map(|suffix| format!("{dataset}{suffix}"))
        })
        .collect::<BTreeSet<_>>();
    (suffixes.len() == 1)
        .then(|| suffixes.into_iter().next())
        .flatten()
}

fn open_rules_label_column_map(
    variables: &[OpenRulesVariable],
) -> BTreeMap<String, Option<String>> {
    let mut labels = BTreeMap::<String, Option<String>>::new();
    for variable in variables {
        let Some(label) = variable.variable.label.as_deref() else {
            continue;
        };
        let key = normalize_open_rules_header_label(label);
        if key.is_empty() {
            continue;
        }
        let name = variable.variable.name.to_ascii_uppercase();
        labels
            .entry(key)
            .and_modify(|existing| {
                if existing.as_deref() != Some(name.as_str()) {
                    *existing = None;
                }
            })
            .or_insert_with(|| Some(name));
    }
    labels
}

fn open_rules_csv_column_name(
    header: &str,
    declared: &BTreeMap<String, OpenRulesVariable>,
    label_columns: &BTreeMap<String, Option<String>>,
) -> String {
    let name = header.trim().to_ascii_uppercase();
    if declared.contains_key(&name) {
        return name;
    }
    label_columns
        .get(&normalize_open_rules_header_label(header))
        .and_then(|name| name.clone())
        .unwrap_or(name)
}

fn normalize_open_rules_embedded_metadata_records(
    mut rows: CsvRecords,
    declared: &BTreeMap<String, OpenRulesVariable>,
    label_columns: &BTreeMap<String, Option<String>>,
) -> CsvRecords {
    if !rows.headers.iter().all(|header| header.trim().is_empty()) {
        return rows;
    }

    let Some(header_row) = rows.records.first() else {
        return rows;
    };
    let mapped_columns = header_row
        .iter()
        .map(|header| open_rules_csv_column_name(header, declared, label_columns))
        .filter(|name| declared.contains_key(name))
        .count();
    if mapped_columns == 0 {
        return rows;
    }

    rows.headers = rows.records.remove(0);
    while rows
        .records
        .first()
        .is_some_and(|row| is_open_rules_embedded_metadata_row(row))
    {
        rows.records.remove(0);
    }
    rows
}

fn is_open_rules_embedded_metadata_row(row: &[String]) -> bool {
    let values = row
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    !values.is_empty()
        && (values.iter().all(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "char" | "character" | "num" | "numeric" | "integer" | "float" | "double"
            )
        }) || values.iter().all(|value| value.parse::<usize>().is_ok()))
}

fn normalize_open_rules_header_label(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase()
}

fn open_rules_cell_value(
    path: &Path,
    dataset: &str,
    variable: &str,
    declared: Option<&OpenRulesVariable>,
    raw: &str,
    row: usize,
    warnings: &mut Vec<LoadDataWarning>,
) -> Value {
    match declared
        .map(|variable| open_rules_variable_kind(&variable.variable))
        .unwrap_or(OpenRulesVariableKind::Character)
    {
        OpenRulesVariableKind::Boolean => {
            let value = raw.trim();
            if value.is_empty() || value == "." {
                return Value::Null;
            }
            parse_csv_bool(value)
                .map(Value::Bool)
                .unwrap_or_else(|| Value::String(raw.to_owned()))
        }
        OpenRulesVariableKind::Numeric => {
            let value = raw.trim();
            if value.is_empty() || value == "." {
                return Value::Null;
            }
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(number_value)
                .unwrap_or_else(|| {
                    warnings.push(LoadDataWarning {
                        path: path.to_path_buf(),
                        kind: LoadDataWarningKind::InvalidNumericValue {
                            dataset: dataset.to_owned(),
                            variable: variable.to_owned(),
                            value: raw.to_owned(),
                            row,
                        },
                    });
                    Value::Null
                })
        }
        OpenRulesVariableKind::Character | OpenRulesVariableKind::Unknown => {
            Value::String(raw.to_owned())
        }
    }
}

fn open_rules_variable_kind(variable: &DatasetVariable) -> OpenRulesVariableKind {
    let Some(variable_type) = variable.variable_type.as_deref() else {
        return OpenRulesVariableKind::Unknown;
    };
    match normalize_metadata_name(variable_type).as_str() {
        "bool" | "boolean" | "logical" => OpenRulesVariableKind::Boolean,
        "char" | "character" | "text" | "string" => OpenRulesVariableKind::Character,
        "num" | "numeric" | "integer" | "float" | "double" => OpenRulesVariableKind::Numeric,
        _ => OpenRulesVariableKind::Unknown,
    }
}

fn ensure_csv_filename(value: &str) -> String {
    let value = value.trim();
    if value.to_ascii_lowercase().ends_with(".csv") {
        value.to_owned()
    } else {
        format!("{value}.csv")
    }
}

fn resolve_open_rules_dataset_path(data_dir: &Path, filename: &str) -> Result<PathBuf> {
    let relative = Path::new(filename);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || relative.file_name().and_then(|name| name.to_str()) != Some(filename)
    {
        return Err(DataError::InvalidDatasetPackage(format!(
            "unsafe dataset filename: {filename}"
        )));
    }

    let root = data_dir.canonicalize().map_err(|source| DataError::Io {
        path: data_dir.to_path_buf(),
        source,
    })?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|source| DataError::Io {
            path: data_dir.join(relative),
            source,
        })?;
    if !path.starts_with(&root) {
        return Err(DataError::InvalidDatasetPackage(format!(
            "dataset filename escapes data dir: {filename}"
        )));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_rules_variable(
        name: &str,
        label: Option<&str>,
        variable_type: Option<&str>,
    ) -> OpenRulesVariable {
        OpenRulesVariable {
            dataset: "AE".to_owned(),
            variable: DatasetVariable {
                name: name.to_owned(),
                label: label.map(str::to_owned),
                variable_type: variable_type.map(str::to_owned),
                length: None,
                extra: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn dataset_descriptor_accepts_manifest_aliases_and_normalizes_names() {
        let row = BTreeMap::from([
            ("File".to_owned(), " ae ".to_owned()),
            ("Domain".to_owned(), " ae ".to_owned()),
            ("Description".to_owned(), "Adverse Events".to_owned()),
        ]);

        let dataset = open_rules_dataset_descriptor(&row).expect("valid dataset descriptor");

        assert_eq!(dataset.filename, "ae.csv");
        assert_eq!(dataset.name, "AE");
        assert_eq!(dataset.label.as_deref(), Some("Adverse Events"));
    }

    #[test]
    fn label_column_map_rejects_ambiguous_labels() {
        let variables = vec![
            open_rules_variable("AETERM", Some("Reported Term"), Some("Char")),
            open_rules_variable("AEDECOD", Some(" Reported   Term "), Some("Char")),
            open_rules_variable("AESEV", Some("Severity"), Some("Char")),
        ];

        let labels = open_rules_label_column_map(&variables);

        assert_eq!(labels.get("REPORTED TERM"), Some(&None));
        assert_eq!(
            labels.get("SEVERITY").and_then(Option::as_deref),
            Some("AESEV")
        );
    }

    #[test]
    fn variable_kind_accepts_case_and_whitespace_but_rejects_unknown_types() {
        let kind = |variable_type| {
            let variable = open_rules_variable("VALUE", None, variable_type);
            open_rules_variable_kind(&variable.variable)
        };

        assert_eq!(kind(Some(" BOOLEAN ")), OpenRulesVariableKind::Boolean);
        assert_eq!(kind(Some("Double")), OpenRulesVariableKind::Numeric);
        assert_eq!(kind(Some("STRING")), OpenRulesVariableKind::Character);
        assert_eq!(kind(Some("date")), OpenRulesVariableKind::Unknown);
        assert_eq!(kind(None), OpenRulesVariableKind::Unknown);
    }
}
