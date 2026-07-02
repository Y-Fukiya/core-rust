use super::*;

pub(super) fn evaluate_unique_set(
    operator: &Operator,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: Option<&Column>,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let group_columns = expand_unique_set_group_columns(operator, comparator, dataset, frame)?;
    let group_column_values = group_columns
        .iter()
        .map(|column| optional_column(frame, column))
        .collect::<Result<Vec<_>>>()?;
    let regex = option_string(&options.extra, "regex")
        .map(|pattern| Regex::new(&pattern))
        .transpose()?;

    let mut counts = std::collections::BTreeMap::<Vec<String>, usize>::new();
    let mut row_keys = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = match target_column {
            Some(column) => ScalarValue::from_any_value(column.get(row)?).to_string(),
            None => "Not in dataset".to_owned(),
        };

        let mut key = Vec::with_capacity(group_columns.len() + 1);
        for column in &group_column_values {
            let value = match column {
                Some(column) => ScalarValue::from_any_value(column.get(row)?)
                    .into_string()
                    .unwrap_or_default(),
                None => "Not in dataset".to_owned(),
            };
            key.push(normalize_unique_set_key_value(&value, regex.as_ref()));
        }
        key.push(normalize_unique_set_key_value(&target, regex.as_ref()));
        *counts.entry(key.clone()).or_default() += 1;
        row_keys.push(Some(key));
    }

    Ok(row_keys
        .into_iter()
        .map(|key| {
            let duplicate =
                key.is_some_and(|key| counts.get(&key).copied().unwrap_or_default() > 1);
            matches!(operator, Operator::IsNotUniqueSet) == duplicate
        })
        .collect())
}

fn normalize_unique_set_key_value(value: &str, regex: Option<&Regex>) -> String {
    regex
        .and_then(|regex| regex.find(value))
        .map(|matched| matched.as_str().to_owned())
        .unwrap_or_else(|| value.to_owned())
}

fn expand_unique_set_group_columns(
    operator: &Operator,
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
) -> Result<Vec<String>> {
    let mut expanded = Vec::new();
    for column in column_name_comparators(operator, comparator)? {
        let column = expand_domain_placeholder(dataset, &column);
        if let Some(dynamic_columns) = dynamic_group_columns(frame, &column)? {
            expanded.extend(
                dynamic_columns
                    .into_iter()
                    .map(|column| expand_domain_placeholder(dataset, &column)),
            );
        } else {
            expanded.push(column);
        }
    }
    Ok(expanded)
}

fn dynamic_group_columns(frame: &DataFrame, column_name: &str) -> Result<Option<Vec<String>>> {
    let Some(column) = optional_column(frame, column_name)? else {
        return Ok(None);
    };
    for row in 0..frame.height() {
        let Some(value) = ScalarValue::from_any_value(column.get(row)?).into_string() else {
            continue;
        };
        if let Some(columns) = parse_group_column_list_literal(&value).filter(|columns| {
            !columns.is_empty()
                && columns.iter().all(|column| {
                    optional_column(frame, column).is_ok_and(|column| column.is_some())
                })
        }) {
            return Ok(Some(columns));
        }
    }
    Ok(None)
}

fn parse_group_column_list_literal(value: &str) -> Option<Vec<String>> {
    let inner = value.trim().strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .filter_map(|part| {
                let column = part.trim().trim_matches('"').trim_matches('\'').trim();
                (!column.is_empty()).then(|| column.to_owned())
            })
            .collect(),
    )
}

pub(super) fn evaluate_not_unique_relationship(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let related_column_name = expand_domain_placeholder(
        dataset,
        &column_name_comparator(&Operator::IsNotUniqueRelationship, comparator)?,
    );
    let related_column = frame
        .column(&related_column_name)
        .map_err(|_| EngineError::MissingColumn(related_column_name.clone()))?;

    let mut related_by_target =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut target_by_related =
        std::collections::BTreeMap::<String, std::collections::BTreeSet<String>>::new();
    let mut row_values = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let related = ScalarValue::from_any_value(related_column.get(row)?);

        let target = relationship_key(&target);
        let related = relationship_key(&related);
        related_by_target
            .entry(target.clone())
            .or_default()
            .insert(related.clone());
        target_by_related
            .entry(related.clone())
            .or_default()
            .insert(target.clone());
        row_values.push(Some((target, related)));
    }

    let direction = option_string(&options.extra, "direction");
    let target_to_comparator_only = direction.as_deref() == Some("target_to_comparator");
    let comparator_to_target_only = direction.as_deref() == Some("comparator_to_target");

    Ok(row_values
        .into_iter()
        .map(|values| {
            let Some((target, related)) = values else {
                return false;
            };
            (!comparator_to_target_only
                && related_by_target
                    .get(&target)
                    .is_some_and(|values| values.len() > 1))
                || (!target_to_comparator_only
                    && target_by_related
                        .get(&related)
                        .is_some_and(|values| values.len() > 1))
        })
        .collect())
}

fn relationship_key(value: &ScalarValue) -> String {
    match value {
        ScalarValue::Null => String::new(),
        ScalarValue::String(value) if value.is_empty() => String::new(),
        value => value.to_string(),
    }
}

pub(super) fn evaluate_inconsistent_across_dataset(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
) -> Result<BooleanMask> {
    let group_columns =
        column_name_comparators(&Operator::IsInconsistentAcrossDataset, comparator)?
            .into_iter()
            .map(|column| expand_domain_placeholder(dataset, &column))
            .collect::<Vec<_>>();
    let group_column_values = group_columns
        .iter()
        .map(|column| optional_column(frame, column))
        .collect::<Result<Vec<_>>>()?;

    let mut target_values_by_key =
        std::collections::BTreeMap::<Vec<String>, std::collections::BTreeSet<String>>::new();
    let mut row_keys = Vec::with_capacity(row_count);

    for row in 0..row_count {
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let mut key = Vec::with_capacity(group_columns.len());
        for column in &group_column_values {
            let value = match column {
                Some(column) => ScalarValue::from_any_value(column.get(row)?)
                    .into_string()
                    .unwrap_or_default(),
                None => "Not in dataset".to_owned(),
            };
            key.push(value);
        }
        target_values_by_key
            .entry(key.clone())
            .or_default()
            .insert(target.to_string());
        row_keys.push(Some(key));
    }

    Ok(row_keys
        .into_iter()
        .map(|key| {
            key.is_some_and(|key| {
                target_values_by_key
                    .get(&key)
                    .map(|values| values.len() > 1)
                    .unwrap_or(false)
            })
        })
        .collect())
}

pub(super) fn evaluate_inconsistent_enumerated_columns(
    frame: &DataFrame,
    row_count: usize,
    target: &str,
) -> Result<BooleanMask> {
    let columns = enumerated_columns(frame, target)?;
    (0..row_count)
        .map(|row| {
            let mut saw_empty = false;
            for column in &columns {
                let value = ScalarValue::from_any_value(column.get(row)?);
                if value.is_empty() {
                    saw_empty = true;
                } else if saw_empty {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .collect()
}

fn enumerated_columns<'a>(frame: &'a DataFrame, target: &str) -> Result<Vec<&'a Column>> {
    let mut columns = Vec::new();
    columns.push(
        frame
            .column(target)
            .map_err(|_| EngineError::MissingColumn(target.to_owned()))?,
    );
    for index in 1.. {
        let name = format!("{target}{index}");
        let Some(column) = optional_column(frame, &name)? else {
            break;
        };
        columns.push(column);
    }
    Ok(columns)
}
