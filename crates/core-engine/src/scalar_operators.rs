use super::*;

pub(super) fn scalar_matches_comparator(
    left: &ScalarValue,
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
    case_insensitive: bool,
    type_insensitive: bool,
) -> Result<bool> {
    match comparator {
        ValueExpr::List(values) => Ok(values.iter().map(json_value_to_scalar).any(|right| {
            scalar_contained_by_value(left, &right, case_insensitive, type_insensitive)
        })),
        _ => {
            let right = resolve_scalar_comparator(comparator, dataset, frame, row)?;
            Ok(scalar_contained_by_value(
                left,
                &right,
                case_insensitive,
                type_insensitive,
            ))
        }
    }
}

pub(super) fn string_prefix(value: &str, len: usize) -> String {
    value.chars().take(len).collect()
}

pub(super) fn string_suffix(value: &str, len: usize) -> String {
    value
        .chars()
        .rev()
        .take(len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

pub(super) fn resolve_scalar_comparator(
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
) -> Result<ScalarValue> {
    match comparator {
        ValueExpr::Literal(value) => Ok(json_value_to_scalar(value)),
        ValueExpr::Null => Ok(ScalarValue::Null),
        ValueExpr::ColumnRef(column_name) => {
            let column_name = expand_domain_placeholder(dataset, column_name);
            let Some(column) = optional_column(frame, &column_name)? else {
                return Ok(ScalarValue::String(column_name));
            };
            Ok(ScalarValue::from_any_value(column.get(row)?))
        }
        ValueExpr::List(_) => Err(EngineError::InvalidComparator {
            operator: "scalar_comparison".to_owned(),
            comparator: comparator.clone(),
        }),
    }
}

pub(super) fn resolve_scalar_list_comparator(
    comparator: &ValueExpr,
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row: usize,
) -> Result<ScalarValue> {
    let ValueExpr::List(values) = comparator else {
        return resolve_scalar_comparator(comparator, dataset, frame, row);
    };

    let mut resolved = Vec::new();
    for value in values {
        if let Some(reference) = value.as_str() {
            if let Some(column) = reference_column(frame, dataset, reference)? {
                let scalar = ScalarValue::from_any_value(column.get(row)?);
                resolved.extend(scalar_list_values(&scalar).map(|value| value.to_string()));
                continue;
            }
        }
        let scalar = json_value_to_scalar(value);
        resolved.extend(scalar_list_values(&scalar).map(|value| value.to_string()));
    }

    Ok(ScalarValue::String(resolved.join("|")))
}

pub(super) fn reference_column<'a>(
    frame: &'a DataFrame,
    dataset: &LoadedDataset,
    value: &str,
) -> Result<Option<&'a Column>> {
    let raw = value.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let mut candidates = vec![raw.to_owned()];
    if let Some(clean) = raw
        .strip_prefix('$')
        .filter(|reference| !reference.is_empty())
    {
        candidates.push(clean.to_owned());
    }

    for candidate in candidates {
        let column_name = expand_domain_placeholder(dataset, &candidate);
        if let Some(column) = optional_column(frame, &column_name)? {
            return Ok(Some(column));
        }
    }

    Ok(None)
}

pub(super) fn expand_domain_placeholder(dataset: &LoadedDataset, name: &str) -> String {
    let Some(suffix) = name.strip_prefix("--") else {
        return name.to_owned();
    };
    let Some(prefix) = domain_prefix(dataset) else {
        return name.to_owned();
    };
    format!("{}{}", prefix, suffix.to_ascii_uppercase())
}

pub(super) fn domain_prefix(dataset: &LoadedDataset) -> Option<String> {
    dataset
        .metadata()
        .domain
        .as_deref()
        .filter(|domain| !domain.trim().is_empty())
        .or_else(|| {
            (!dataset.metadata().name.trim().is_empty()).then_some(dataset.metadata().name.as_str())
        })
        .map(|domain| domain.trim().to_ascii_uppercase())
}

pub(super) fn json_value_to_scalar(value: &Value) -> ScalarValue {
    match value {
        Value::Null => ScalarValue::Null,
        Value::Bool(value) => ScalarValue::Bool(*value),
        Value::Number(value) => value
            .as_f64()
            .map(ScalarValue::Number)
            .unwrap_or_else(|| ScalarValue::String(value.to_string())),
        Value::String(value) => ScalarValue::String(value.clone()),
        other => ScalarValue::String(other.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum ScalarValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

impl ScalarValue {
    pub(super) fn from_any_value(value: AnyValue<'_>) -> Self {
        if value.is_null() {
            return Self::Null;
        }

        if let Some(value) = value.extract_bool() {
            return Self::Bool(value);
        }

        if let Some(value) = value.extract_str() {
            return Self::String(value.to_owned());
        }

        if let Some(value) = value.extract::<f64>() {
            return Self::Number(value);
        }

        Self::String(value.to_string())
    }

    pub(super) fn is_empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::String(value) => value.is_empty(),
            _ => false,
        }
    }

    pub(super) fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    pub(super) fn as_type_insensitive_number(&self) -> Option<f64> {
        match self {
            Self::Number(value) if value.is_finite() => Some(*value),
            Self::String(value) => value
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite()),
            _ => None,
        }
    }

    pub(super) fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value.as_str()),
            _ => None,
        }
    }

    pub(super) fn into_string(self) -> Option<String> {
        match self {
            Self::Null => None,
            Self::Bool(value) => Some(value.to_string()),
            Self::Number(value) => Some(value.to_string()),
            Self::String(value) => Some(value),
        }
    }
}

impl std::fmt::Display for ScalarValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Null => f.write_str("null"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(value) => write!(f, "{value}"),
            Self::String(value) => f.write_str(value),
        }
    }
}

pub(super) fn scalar_equal_with_mode(
    left: &ScalarValue,
    right: &ScalarValue,
    case_insensitive: bool,
    type_insensitive: bool,
) -> bool {
    if type_insensitive {
        if let (Some(left), Some(right)) = (
            left.as_type_insensitive_number(),
            right.as_type_insensitive_number(),
        ) {
            return left == right;
        }
    }

    match (left, right) {
        (ScalarValue::Null, ScalarValue::Null) => true,
        (ScalarValue::Null, _) | (_, ScalarValue::Null) => false,
        (ScalarValue::Bool(left), ScalarValue::Bool(right)) => left == right,
        (ScalarValue::Bool(left), ScalarValue::String(right))
        | (ScalarValue::String(right), ScalarValue::Bool(left)) => {
            match right.trim().to_ascii_lowercase().as_str() {
                "true" => *left,
                "false" => !*left,
                _ => false,
            }
        }
        (ScalarValue::String(left), ScalarValue::String(right)) if case_insensitive => {
            left.eq_ignore_ascii_case(right)
        }
        (ScalarValue::String(left), ScalarValue::String(right)) => left == right,
        (ScalarValue::Number(left), ScalarValue::Number(right)) => left == right,
        _ => false,
    }
}

pub(super) fn scalar_contained_by_value(
    left: &ScalarValue,
    right: &ScalarValue,
    case_insensitive: bool,
    type_insensitive: bool,
) -> bool {
    if scalar_equal_with_mode(left, right, case_insensitive, type_insensitive) {
        return true;
    }

    let ScalarValue::String(right) = right else {
        return false;
    };
    if !right.contains('|') {
        return false;
    }

    right.split('|').any(|part| {
        let part = part.trim();
        scalar_equal_with_mode(
            left,
            &ScalarValue::String(part.to_owned()),
            case_insensitive,
            type_insensitive,
        ) || scalar_string_equal_with_mode(left, part, case_insensitive)
    })
}

pub(super) fn scalar_contains_all(
    left: &ScalarValue,
    right: &ScalarValue,
    case_insensitive: bool,
) -> bool {
    scalar_list_values(right).all(|value| {
        scalar_contained_by_value(&value, left, case_insensitive, false)
            || scalar_string_equal_with_mode(left, &value.to_string(), case_insensitive)
    })
}

pub(super) fn scalar_shares_no_elements_with(left: &ScalarValue, right: &ScalarValue) -> bool {
    !scalar_list_values(left).any(|left_value| {
        scalar_list_values(right)
            .any(|right_value| scalar_equal_with_mode(&left_value, &right_value, false, false))
    })
}

pub(super) fn scalar_is_ordered_subset_of(left: &ScalarValue, right: &ScalarValue) -> bool {
    let left_values = scalar_list_values(left)
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    let right_values = scalar_list_values(right)
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if left_values.is_empty() {
        return true;
    }

    let mut right_index = 0;
    for left_value in left_values {
        let Some(next_index) = right_values[right_index..]
            .iter()
            .position(|right_value| right_value == &left_value)
        else {
            return false;
        };
        right_index += next_index + 1;
    }
    true
}

pub(super) fn scalar_list_values(
    value: &ScalarValue,
) -> Box<dyn Iterator<Item = ScalarValue> + '_> {
    match value {
        ScalarValue::String(value) if value.contains('|') => Box::new(
            value
                .split('|')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| ScalarValue::String(part.to_owned())),
        ),
        ScalarValue::String(value) if value.trim().is_empty() => Box::new(std::iter::empty()),
        other => Box::new(std::iter::once(other.clone())),
    }
}

pub(super) fn string_contains_value(haystack: &str, needle: &str, case_insensitive: bool) -> bool {
    if haystack.contains('|') {
        return haystack
            .split('|')
            .map(str::trim)
            .any(|part| string_equal_with_mode(part, needle, case_insensitive));
    }

    if case_insensitive {
        haystack
            .to_ascii_lowercase()
            .contains(&needle.to_ascii_lowercase())
    } else {
        haystack.contains(needle)
    }
}

pub(super) fn scalar_string_equal_with_mode(
    left: &ScalarValue,
    right: &str,
    case_insensitive: bool,
) -> bool {
    string_equal_with_mode(&left.to_string(), right, case_insensitive)
}

pub(super) fn string_equal_with_mode(left: &str, right: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}
