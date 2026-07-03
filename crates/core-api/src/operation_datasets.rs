use std::collections::BTreeSet;

use core_data::{dataset_column_values, DataError, LoadedDataset};
use core_rule_model::OperationSpec;
use serde_json::Value;

use crate::json_values::json_scalar_string;
use crate::operation_execution::operation_column_values;
use crate::operation_fields::{
    normalize_operation_key, operation_value, string_field, string_list_field,
};
use crate::operation_references::{
    operation_reference_values, optional_operation_reference_values,
};
use crate::static_codelists::{
    ddf_valid_codelist_dates, static_codelist, static_codelist_matches_version,
    static_codelist_term_by_code, static_codelist_term_by_pref_term, static_codelist_term_by_value,
    static_codelist_term_matches_version, valid_codelist_dates,
};
use crate::{
    dataset_has_variable, derive_column_from_values_with_aliases, expand_dataset_domain_placeholder,
};

pub(crate) fn derive_domain_label_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
    prefer_domain_name: bool,
) -> std::result::Result<LoadedDataset, DataError> {
    let label = if prefer_domain_name {
        domain_name_value(dataset)
    } else {
        domain_label_value(dataset)
    };
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(label.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_study_domains_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let domains = all_datasets
        .iter()
        .flat_map(|dataset| {
            [
                dataset.metadata.domain.as_deref(),
                Some(dataset.metadata.name.as_str()),
            ]
        })
        .flatten()
        .filter_map(|value| {
            let value = value.trim();
            (!value.is_empty()).then(|| value.to_ascii_uppercase())
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(domains.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_variable_count_dataset(
    dataset: &LoadedDataset,
    all_datasets: &[LoadedDataset],
    source_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "variable_count operation requires a source variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "variable_count operation requires an output column".to_owned(),
        ));
    }

    let count = all_datasets
        .iter()
        .filter(|candidate| {
            let column = expand_dataset_domain_placeholder(candidate, source_column);
            dataset_has_variable(candidate, &column)
        })
        .count() as i64;
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::Number(serde_json::Number::from(count)))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_study_day_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    reference_column: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires a source date variable".to_owned(),
        ));
    }
    if reference_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires a reference date variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "dy operation requires an output column".to_owned(),
        ));
    }

    let source_column = expand_dataset_domain_placeholder(dataset, source_column);
    let source_dates = dataset_column_values(dataset, &source_column)?;
    let reference_dates = dataset_column_values(dataset, reference_column)?;
    let values = source_dates
        .iter()
        .zip(reference_dates.iter())
        .map(|(source, reference)| {
            study_day_value(
                json_scalar_string(source).as_deref(),
                json_scalar_string(reference).as_deref(),
            )
            .map(|value| Value::Number(serde_json::Number::from(value)))
            .unwrap_or(Value::Null)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn study_day_value(source: Option<&str>, reference: Option<&str>) -> Option<i64> {
    let source = days_from_study_date(source?)?;
    let reference = days_from_study_date(reference?)?;
    let diff = source - reference;
    Some(if diff >= 0 { diff + 1 } else { diff })
}

fn days_from_study_date(value: &str) -> Option<i64> {
    let date = value.trim().get(..10)?;
    let year = parse_fixed_i32(date.get(0..4)?)?;
    let separator_1 = date.get(4..5)?;
    let month = parse_fixed_u32(date.get(5..7)?)?;
    let separator_2 = date.get(7..8)?;
    let day = parse_fixed_u32(date.get(8..10)?)?;
    if separator_1 != "-" || separator_2 != "-" {
        return None;
    }
    if !(1..=12).contains(&month) || day == 0 || day > days_in_study_month(year, month) {
        return None;
    }

    Some(days_from_civil(year, month, day))
}

fn parse_fixed_i32(value: &str) -> Option<i32> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<i32>().ok())
        .flatten()
}

fn parse_fixed_u32(value: &str) -> Option<u32> {
    value
        .chars()
        .all(|character| character.is_ascii_digit())
        .then(|| value.parse::<u32>().ok())
        .flatten()
}

fn days_in_study_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_study_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_study_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let adjusted_year = year - i32::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = (adjusted_year - era * 400) as i64;
    let month = month as i64;
    let day = day as i64;
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era as i64 * 146_097 + day_of_era
}

pub(crate) fn derive_metadata_dataset(
    dataset: &LoadedDataset,
    field: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let value = match normalize_operation_key(field).as_str() {
        "dataset_name" | "name" => dataset.metadata.name.clone(),
        "domain" => dataset
            .metadata
            .domain
            .clone()
            .unwrap_or_else(|| dataset.metadata.name.clone()),
        "label" | "dataset_label" => dataset.metadata.label.clone().unwrap_or_default(),
        other => {
            return Err(DataError::InvalidDatasetPackage(format!(
                "unsupported metadata field: {other}"
            )));
        }
    };
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(value.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_valid_codelist_dates_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let joined = valid_codelist_dates_for_operation(operation).join("|");
    let values = (0..dataset.summary().row_count)
        .map(|_| Value::String(joined.clone()))
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn valid_codelist_dates_for_operation(
    operation: &OperationSpec,
) -> &'static [&'static str] {
    let package_types =
        string_list_field(operation, &["ct_package_types", "ct_package_type"]).unwrap_or_default();
    if package_types
        .iter()
        .any(|package_type| package_type.eq_ignore_ascii_case("DDF"))
    {
        return ddf_valid_codelist_dates();
    }
    valid_codelist_dates()
}

pub(crate) fn derive_mapped_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let mappings = operation_value(operation, &["map"])
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut values = Vec::with_capacity(dataset.summary().row_count);

    for row in 0..dataset.summary().row_count {
        let mut mapped = String::new();
        for mapping in &mappings {
            let Some(object) = mapping.as_object() else {
                continue;
            };
            let output = object
                .get("output")
                .and_then(json_scalar_string)
                .unwrap_or_default();
            if output.is_empty() {
                continue;
            }
            let matched = object
                .iter()
                .filter(|(key, _value)| key.as_str() != "output")
                .all(|(key, expected)| {
                    operation_column_values(dataset, key)
                        .ok()
                        .and_then(|values| values.get(row).and_then(json_scalar_string))
                        .zip(json_scalar_string(expected))
                        .is_some_and(|(actual, expected)| actual == expected)
                });
            if matched {
                mapped = output;
                break;
            }
        }
        values.push(Value::String(mapped));
    }

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_codelist_extensible_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let codelist_codes = operation_reference_values(dataset, operation, "codelist_code")?;
    let codelist_versions = optional_operation_reference_values(dataset, operation, "version")?;
    let values = codelist_codes
        .iter()
        .enumerate()
        .map(|(row, code)| {
            let version = codelist_versions
                .as_ref()
                .and_then(|values| values.get(row))
                .map(String::as_str);
            if !static_codelist_matches_version(code, version) {
                return Value::Null;
            }
            match static_codelist(code).map(|codelist| codelist.extensible) {
                Some(value) => Value::Bool(value),
                None => Value::Null,
            }
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_codelist_terms_dataset(
    dataset: &LoadedDataset,
    operation: &OperationSpec,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let codelist_codes = if operation_value(operation, &["codelist_code"]).is_some() {
        operation_reference_values(dataset, operation, "codelist_code")?
    } else {
        let literal = string_list_field(operation, &["codelists"])
            .and_then(|values| values.first().cloned())
            .unwrap_or_default();
        vec![literal; dataset.summary().row_count]
    };
    let return_type = string_field(operation, &["returntype", "return_type"])
        .unwrap_or_else(|| "value".to_owned());
    let term_code = optional_operation_reference_values(dataset, operation, "term_code")?;
    let term_pref_term = optional_operation_reference_values(dataset, operation, "term_pref_term")?;
    let term_value = optional_operation_reference_values(dataset, operation, "term_value")?;
    let term_version = optional_operation_reference_values(dataset, operation, "version")?;

    let values = (0..dataset.summary().row_count)
        .map(|row| {
            let Some(codelist_code) = codelist_codes.get(row) else {
                return Value::String(String::new());
            };
            let Some(codelist) = static_codelist(codelist_code) else {
                return Value::String(String::new());
            };
            let version = term_version
                .as_ref()
                .and_then(|values| values.get(row))
                .map(String::as_str);
            if term_code.is_none() && term_pref_term.is_none() && term_value.is_none() {
                let values = codelist
                    .terms
                    .iter()
                    .filter(|term| {
                        static_codelist_term_matches_version(codelist_code, term, version)
                    })
                    .map(|term| term.value)
                    .collect::<Vec<_>>()
                    .join("|");
                return Value::String(values);
            }
            let term = term_code
                .as_ref()
                .and_then(|values| values.get(row))
                .and_then(|code| {
                    static_codelist_term_by_code(codelist_code, &codelist, code, version)
                })
                .or_else(|| {
                    term_pref_term
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|pref_term| {
                            static_codelist_term_by_pref_term(
                                codelist_code,
                                &codelist,
                                pref_term,
                                version,
                            )
                        })
                })
                .or_else(|| {
                    term_value
                        .as_ref()
                        .and_then(|values| values.get(row))
                        .and_then(|value| {
                            static_codelist_term_by_value(codelist_code, &codelist, value, version)
                        })
                });
            let Some(term) = term else {
                return Value::String(String::new());
            };
            if !static_codelist_term_matches_version(codelist_code, term, version) {
                return Value::String(String::new());
            }
            let value = match normalize_operation_key(&return_type).as_str() {
                "code" => term.code,
                "pref_term" | "preferred_term" => term.pref_term,
                "value" | "submission_value" => term.value,
                _ => "",
            };
            Value::String(value.to_owned())
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_split_by_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    delimiter: &str,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let source_values = operation_column_values(dataset, source_column)?;
    let values = source_values
        .iter()
        .map(|value| {
            let Some(value) = json_scalar_string(value) else {
                return Value::String(String::new());
            };
            let parts = value
                .split(delimiter)
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("|");
            Value::String(parts)
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

pub(crate) fn derive_parent_model_column_order_dataset(
    dataset: &LoadedDataset,
    column_name: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    let rdomains = operation_column_values(dataset, "RDOMAIN")?;
    let values = rdomains
        .iter()
        .map(|value| {
            let domain = json_scalar_string(value).unwrap_or_default();
            Value::String(parent_model_columns(&domain).unwrap_or_default().join("|"))
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn parent_model_columns(domain: &str) -> Option<&'static [&'static str]> {
    match domain.trim().to_ascii_uppercase().as_str() {
        "AE" => Some(AE_MODEL_COLUMNS),
        "LB" => Some(LB_MODEL_COLUMNS),
        _ => None,
    }
}

const AE_MODEL_COLUMNS: &[&str] = &[
    "STUDYID", "AEGRPID", "AEREFID", "AERECID", "AESPID", "DOMAIN", "USUBJID", "AESEQ", "AETERM",
    "AEHLGT", "AEHLGTCD", "AECAT", "AESCAT", "AEPRESP", "AEOCCUR", "AEREASOC", "AESTAT",
    "AEREASND", "AEBODSYS", "AEMODIFY", "AEBDSYCD", "AESOC", "AESOCCD", "AELOC", "AELAT", "AEDIR",
    "AEPORTOT", "AEPARTY", "AELLT", "AEPRTYID", "AESEV", "AESER", "AEACN", "AEACNOTH", "AEACNDEV",
    "AEREL", "AERLDEV", "AERELNST", "AEPATT", "AELLTCD", "AEOUT", "AESCONG", "AESDISAB", "AESDTH",
    "AESHOSP", "AESLIFE", "AESOD", "AESMIE", "AESINTV", "AEDECOD", "AECONTRT", "AETOX", "AETOXGR",
    "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH", "AEDTC", "AESTDTC", "AEENDTC", "AEDY",
    "AESTDY", "AEENDY", "AEDUR",
];

const LB_MODEL_COLUMNS: &[&str] = &[
    "STUDYID", "LBGRPID", "LBREFID", "LBRECID", "LBSPID", "DOMAIN", "USUBJID", "LBSEQ", "LBTESTCD",
    "LBTEST", "LBCAT", "LBSCAT", "LBORRES", "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC",
    "LBSTRESN", "LBSTRESU", "LBSTNRLO", "LBSTNRHI", "LBSTNRC", "LBNRIND", "LBSTAT", "LBREASND",
    "LBNAM", "LBLOINC", "LBSPEC", "LBSPCCND", "LBSPCUFL", "LBLOC", "LBLAT", "LBDIR", "LBPORTOT",
    "LBMETHOD", "LBANMETH", "LBLOBXFL", "LBBLFL", "LBFAST", "LBDRVFL", "LBTOX", "LBTOXGR",
    "LBCLSIG", "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH", "LBDTC", "LBSTDTC", "LBENDTC",
    "LBDY", "LBSTDY", "LBENDY", "LBTPT", "LBTPTNUM", "LBELTM", "LBTPTREF", "LBRFTDTC", "LBPTFL",
    "LBPDUR",
];

pub(crate) fn derive_xhtml_errors_dataset(
    dataset: &LoadedDataset,
    source_column: &str,
    column_name: &str,
    namespace: &str,
) -> std::result::Result<LoadedDataset, DataError> {
    if source_column.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "get_xhtml_errors operation requires a source variable".to_owned(),
        ));
    }
    if column_name.trim().is_empty() {
        return Err(DataError::InvalidDatasetPackage(
            "get_xhtml_errors operation requires an output column".to_owned(),
        ));
    }

    let source_values = operation_column_values(dataset, source_column)
        .unwrap_or_else(|_| vec![Value::Null; dataset.summary().row_count]);
    let values = source_values
        .iter()
        .map(|value| {
            let Some(text) = json_scalar_string(value) else {
                return Value::Null;
            };
            if xhtml_fragment_errors(&text, namespace).is_empty() {
                Value::Null
            } else {
                Value::String("invalid xhtml".to_owned())
            }
        })
        .collect::<Vec<_>>();

    derive_column_from_values_with_aliases(dataset, column_name, &values)
}

fn xhtml_fragment_errors(text: &str, namespace: &str) -> Vec<String> {
    if text.trim().is_empty() {
        return Vec::new();
    }

    let wrapped = format!(r#"<root xmlns:usdm="{namespace}">{text}</root>"#);
    let Ok(document) = roxmltree::Document::parse(&wrapped) else {
        return vec!["invalid xhtml".to_owned()];
    };

    let mut errors = Vec::new();
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.parent().is_none() {
            continue;
        }
        let name = node.tag_name().name();
        let ns = node.tag_name().namespace();
        if ns == Some(namespace) {
            if name != "tag" && name != "ref" {
                errors.push(format!("unsupported usdm element {name}"));
                continue;
            }
            if name == "tag" && !node.has_attribute("name") {
                errors.push("usdm:tag requires name".to_owned());
            }
            if name == "ref" && !node.has_attribute("id") {
                errors.push("usdm:ref requires id".to_owned());
            }
            if node.children().any(|child| {
                child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty())
            }) {
                errors.push(format!("usdm:{name} must be empty"));
            }
            for attribute in node.attributes() {
                if !is_allowed_usdm_xhtml_attribute(name, attribute.name()) {
                    errors.push(format!(
                        "unsupported usdm:{name} attribute {}",
                        attribute.name()
                    ));
                }
            }
            continue;
        }

        if !is_allowed_xhtml_element(name) {
            errors.push(format!("unsupported xhtml element {name}"));
            continue;
        }
        if matches!(name, "ul" | "ol") {
            for child in node.children().filter(|child| child.is_element()) {
                if child.tag_name().name() != "li" {
                    errors.push(format!(
                        "unsupported xhtml element {} under {name}",
                        child.tag_name().name()
                    ));
                }
            }
        }

        for attribute in node.attributes() {
            if !is_allowed_xhtml_attribute(name, attribute.name()) {
                errors.push(format!(
                    "unsupported xhtml attribute {} on {name}",
                    attribute.name()
                ));
            }
        }
    }

    errors
}

fn is_allowed_xhtml_element(name: &str) -> bool {
    matches!(
        name,
        "root"
            | "a"
            | "b"
            | "br"
            | "div"
            | "em"
            | "i"
            | "img"
            | "li"
            | "ol"
            | "p"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "table"
            | "tbody"
            | "td"
            | "th"
            | "thead"
            | "tr"
            | "u"
            | "ul"
    )
}

fn is_allowed_xhtml_attribute(element: &str, attribute: &str) -> bool {
    matches!(attribute, "class" | "id" | "style")
        || (element == "a" && matches!(attribute, "href" | "title"))
        || (element == "img" && matches!(attribute, "alt" | "src"))
        || ((element == "td" || element == "th") && matches!(attribute, "colspan" | "rowspan"))
}

fn is_allowed_usdm_xhtml_attribute(element: &str, attribute: &str) -> bool {
    match element {
        "tag" => attribute == "name",
        "ref" => matches!(attribute, "attribute" | "id" | "klass"),
        _ => false,
    }
}

fn domain_label_value(dataset: &LoadedDataset) -> String {
    if let Some(label) = dataset.metadata.label.as_ref() {
        if !label.trim().is_empty() {
            return label.trim().to_owned();
        }
    }
    if let Some(domain) = dataset.metadata.domain.as_ref() {
        if !domain.trim().is_empty() {
            return domain.trim().to_owned();
        }
    }
    if !dataset.metadata.name.trim().is_empty() {
        return dataset.metadata.name.trim().to_owned();
    }
    String::new()
}

fn domain_name_value(dataset: &LoadedDataset) -> String {
    if let Some(domain) = dataset.metadata.domain.as_ref() {
        if !domain.trim().is_empty() {
            return domain.trim().to_owned();
        }
    }
    if !dataset.metadata.name.trim().is_empty() {
        return dataset.metadata.name.trim().to_owned();
    }
    if let Some(label) = dataset.metadata.label.as_ref() {
        if !label.trim().is_empty() {
            return label.trim().to_owned();
        }
    }
    String::new()
}
