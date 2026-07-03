use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::usdm_values::{json_string, value_string};

pub(crate) fn collect_usdm_abbreviation_rows(
    value: &Value,
    rows: &mut Vec<BTreeMap<String, Value>>,
) {
    let Some(versions) = value
        .get("study")
        .and_then(|study| study.get("versions"))
        .and_then(Value::as_array)
    else {
        return;
    };

    for (version_index, version) in versions.iter().enumerate() {
        let Some(abbreviations) = version.get("abbreviations").and_then(Value::as_array) else {
            continue;
        };
        let mut version_rows = abbreviations
            .iter()
            .enumerate()
            .map(|(abbreviation_index, abbreviation)| {
                usdm_abbreviation_row(abbreviation, version, version_index, abbreviation_index)
            })
            .collect::<Vec<_>>();
        apply_abbreviation_duplicate_flags(&mut version_rows);
        rows.extend(version_rows);
    }
}

fn usdm_abbreviation_row(
    abbreviation: &Value,
    version: &Value,
    version_index: usize,
    abbreviation_index: usize,
) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    row.insert(
        "path".to_owned(),
        Value::String(format!(
            "/study/versions/{version_index}/abbreviations/{abbreviation_index}"
        )),
    );
    row.insert(
        "instanceType".to_owned(),
        json_string(abbreviation.get("instanceType")),
    );
    row.insert("id".to_owned(), json_string(abbreviation.get("id")));
    row.insert("StudyVersion.id".to_owned(), json_string(version.get("id")));
    row.insert(
        "StudyVersion.versionIdentifier".to_owned(),
        json_string(version.get("versionIdentifier")),
    );
    row.insert(
        "abbreviatedText".to_owned(),
        json_string(abbreviation.get("abbreviatedText")),
    );
    row.insert(
        "expandedText".to_owned(),
        json_string(abbreviation.get("expandedText")),
    );
    row
}

fn apply_abbreviation_duplicate_flags(rows: &mut [BTreeMap<String, Value>]) {
    let mut expanded_counts: HashMap<String, usize> = HashMap::new();
    let mut abbreviated_counts: HashMap<String, usize> = HashMap::new();
    for row in rows.iter() {
        let expanded = row
            .get("expandedText")
            .and_then(value_string)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !expanded.is_empty() {
            *expanded_counts.entry(expanded).or_insert(0) += 1;
        }
        let abbreviated = row
            .get("abbreviatedText")
            .and_then(value_string)
            .unwrap_or_default();
        if !abbreviated.is_empty() {
            *abbreviated_counts.entry(abbreviated).or_insert(0) += 1;
        }
    }
    for row in rows.iter_mut() {
        let expanded = row
            .get("expandedText")
            .and_then(value_string)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let expanded_duplicate = !expanded.is_empty()
            && expanded_counts
                .get(&expanded)
                .is_some_and(|count| *count > 1);
        let abbreviated = row
            .get("abbreviatedText")
            .and_then(value_string)
            .unwrap_or_default();
        let abbreviated_duplicate = !abbreviated.is_empty()
            && abbreviated_counts
                .get(&abbreviated)
                .is_some_and(|count| *count > 1);
        row.insert(
            "abbreviation_expanded_text_duplicate".to_owned(),
            Value::Bool(expanded_duplicate),
        );
        row.insert(
            "abbreviation_text_duplicate".to_owned(),
            Value::Bool(abbreviated_duplicate),
        );
    }
}
