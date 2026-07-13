use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use polars::prelude::*;

use crate::{DataError, Result};

pub(crate) fn row_key(frame: &DataFrame, keys: &[String], row: usize) -> Result<Vec<RowKeyValue>> {
    keys.iter()
        .map(|key| cell_to_key(frame, key, row))
        .collect()
}

pub(crate) fn row_key_contains_null(key: &[RowKeyValue]) -> bool {
    key.iter().any(|value| matches!(value, RowKeyValue::Null))
}

#[derive(Debug, Clone)]
pub(crate) enum RowKeyValue {
    Null,
    Bool(bool),
    SignedInteger(i64),
    UnsignedInteger(u64),
    Float(NumberKey),
    String(String),
}

impl RowKeyValue {
    fn from_any_value(value: AnyValue<'_>) -> Self {
        if value.is_null() {
            return Self::Null;
        }
        if let Some(value) = value.extract_bool() {
            return Self::Bool(value);
        }
        if let Some(value) = value.extract_str() {
            return Self::String(value.to_owned());
        }
        match value {
            AnyValue::Int8(value) => return Self::SignedInteger(value.into()),
            AnyValue::Int16(value) => return Self::SignedInteger(value.into()),
            AnyValue::Int32(value) => return Self::SignedInteger(value.into()),
            AnyValue::Int64(value) => return Self::SignedInteger(value),
            AnyValue::UInt8(value) => return Self::UnsignedInteger(value.into()),
            AnyValue::UInt16(value) => return Self::UnsignedInteger(value.into()),
            AnyValue::UInt32(value) => return Self::UnsignedInteger(value.into()),
            AnyValue::UInt64(value) => return Self::UnsignedInteger(value),
            AnyValue::Float32(value) => return Self::Float(NumberKey::new(value.into())),
            AnyValue::Float64(value) => return Self::Float(NumberKey::new(value)),
            _ => {}
        }
        Self::String(value.to_string())
    }

    fn numeric_cmp(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::SignedInteger(left), Self::SignedInteger(right)) => Some(left.cmp(right)),
            (Self::UnsignedInteger(left), Self::UnsignedInteger(right)) => Some(left.cmp(right)),
            (Self::Float(left), Self::Float(right)) => Some(left.cmp(right)),
            (Self::SignedInteger(left), Self::UnsignedInteger(right)) => {
                Some(compare_i64_u64(*left, *right))
            }
            (Self::UnsignedInteger(left), Self::SignedInteger(right)) => {
                Some(compare_i64_u64(*right, *left).reverse())
            }
            (Self::SignedInteger(left), Self::Float(right)) => {
                Some(compare_i64_f64(*left, right.value()))
            }
            (Self::Float(left), Self::SignedInteger(right)) => {
                Some(compare_i64_f64(*right, left.value()).reverse())
            }
            (Self::UnsignedInteger(left), Self::Float(right)) => {
                Some(compare_u64_f64(*left, right.value()))
            }
            (Self::Float(left), Self::UnsignedInteger(right)) => {
                Some(compare_u64_f64(*right, left.value()).reverse())
            }
            _ => None,
        }
    }
}

impl PartialEq for RowKeyValue {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for RowKeyValue {}

impl Hash for RowKeyValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Null => {
                0_u8.hash(state);
            }
            Self::Bool(value) => {
                1_u8.hash(state);
                value.hash(state);
            }
            Self::SignedInteger(value) => {
                hash_numeric_integer(*value, state);
            }
            Self::UnsignedInteger(value) => {
                hash_numeric_unsigned(*value, state);
            }
            Self::Float(value) => {
                hash_numeric_float(*value, state);
            }
            Self::String(value) => {
                3_u8.hash(state);
                value.hash(state);
            }
        }
    }
}

impl PartialOrd for RowKeyValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RowKeyValue {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Bool(left), Self::Bool(right)) => left.cmp(right),
            (Self::Bool(_), _) => Ordering::Less,
            (_, Self::Bool(_)) => Ordering::Greater,
            (Self::String(left), Self::String(right)) => left.cmp(right),
            (Self::String(_), _) => Ordering::Greater,
            (_, Self::String(_)) => Ordering::Less,
            _ => self
                .numeric_cmp(other)
                .expect("non-string non-bool non-null row keys are numeric"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NumberKey(u64);

impl NumberKey {
    fn new(value: f64) -> Self {
        let value = if value == 0.0 { 0.0 } else { value };
        Self(value.to_bits())
    }

    fn value(self) -> f64 {
        f64::from_bits(self.0)
    }
}

impl PartialOrd for NumberKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NumberKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value().total_cmp(&other.value())
    }
}

fn compare_i64_u64(left: i64, right: u64) -> Ordering {
    if left < 0 {
        return Ordering::Less;
    }
    (left as u64).cmp(&right)
}

fn compare_i64_f64(left: i64, right: f64) -> Ordering {
    if let Some(right) = integer_i64_from_f64(right) {
        return left.cmp(&right);
    }
    if right.is_nan() {
        return 0.0_f64.total_cmp(&right);
    }
    if right == f64::INFINITY {
        return Ordering::Less;
    }
    if right == f64::NEG_INFINITY || right < i64::MIN as f64 {
        return Ordering::Greater;
    }
    if right >= i64::MAX as f64 {
        return Ordering::Less;
    }
    let floor = right.floor() as i64;
    if left <= floor {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

fn compare_u64_f64(left: u64, right: f64) -> Ordering {
    if let Some(right) = integer_u64_from_f64(right) {
        return left.cmp(&right);
    }
    if right.is_nan() {
        return 0.0_f64.total_cmp(&right);
    }
    if right == f64::INFINITY {
        return Ordering::Less;
    }
    if right == f64::NEG_INFINITY || right < 0.0 {
        return Ordering::Greater;
    }
    if right >= u64::MAX as f64 {
        return Ordering::Less;
    }
    let floor = right.floor() as u64;
    if left <= floor {
        Ordering::Less
    } else {
        Ordering::Greater
    }
}

fn integer_i64_from_f64(value: f64) -> Option<i64> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    if value < i64::MIN as f64 || value >= i64::MAX as f64 {
        return None;
    }
    let integer = value as i64;
    ((integer as f64) == value).then_some(integer)
}

fn integer_u64_from_f64(value: f64) -> Option<u64> {
    if !value.is_finite() || value.fract() != 0.0 || value < 0.0 {
        return None;
    }
    if value >= u64::MAX as f64 {
        return None;
    }
    let integer = value as u64;
    ((integer as f64) == value).then_some(integer)
}

fn hash_numeric_integer<H: Hasher>(value: i64, state: &mut H) {
    if value < 0 {
        2_u8.hash(state);
        value.hash(state);
    } else {
        hash_numeric_unsigned(value as u64, state);
    }
}

fn hash_numeric_unsigned<H: Hasher>(value: u64, state: &mut H) {
    2_u8.hash(state);
    value.hash(state);
}

fn hash_numeric_float<H: Hasher>(value: NumberKey, state: &mut H) {
    let value = value.value();
    if let Some(value) = integer_i64_from_f64(value) {
        hash_numeric_integer(value, state);
    } else if let Some(value) = integer_u64_from_f64(value) {
        hash_numeric_unsigned(value, state);
    } else {
        2_u8.hash(state);
        value.to_bits().hash(state);
    }
}

fn cell_to_key(frame: &DataFrame, column_name: &str, row: usize) -> Result<RowKeyValue> {
    let column = frame
        .column(column_name)
        .map_err(|source| DataError::Polars {
            path: PathBuf::from(column_name),
            source,
        })?;
    let value = column.get(row).map_err(|source| DataError::Polars {
        path: PathBuf::from(column_name),
        source,
    })?;
    Ok(RowKeyValue::from_any_value(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::{hash_map::DefaultHasher, BTreeSet, HashSet};

    fn row_key_value_strategy() -> impl Strategy<Value = RowKeyValue> {
        prop_oneof![
            Just(RowKeyValue::Null),
            any::<bool>().prop_map(RowKeyValue::Bool),
            prop::sample::select(vec![
                i64::MIN,
                -9_007_199_254_740_993,
                -42,
                -1,
                0,
                1,
                42,
                9_007_199_254_740_992,
                9_007_199_254_740_993,
                i64::MAX,
            ])
            .prop_map(RowKeyValue::SignedInteger),
            prop::sample::select(vec![
                0_u64,
                1,
                42,
                9_007_199_254_740_992,
                9_007_199_254_740_993,
                u64::MAX,
            ])
            .prop_map(RowKeyValue::UnsignedInteger),
            prop::sample::select(vec![
                f64::from_bits(0xfff8_0000_0000_0000),
                f64::NEG_INFINITY,
                -9_007_199_254_740_992.0,
                -42.0,
                -0.5,
                -0.0,
                0.0,
                0.5,
                42.0,
                9_007_199_254_740_992.0,
                f64::INFINITY,
                f64::NAN,
            ])
            .prop_map(|value| RowKeyValue::Float(NumberKey::new(value))),
            "[A-Z0-9_]{0,8}".prop_map(RowKeyValue::String),
        ]
    }

    fn hash_value(value: &RowKeyValue) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn row_key_numeric_equality_hashes_integral_values_consistently_across_types() {
        let values = [
            RowKeyValue::SignedInteger(42),
            RowKeyValue::UnsignedInteger(42),
            RowKeyValue::Float(NumberKey::new(42.0)),
            RowKeyValue::Float(NumberKey::new(42.0_f32.into())),
        ];

        for left in &values {
            for right in &values {
                assert_eq!(left, right);
            }
        }

        assert_eq!(values.into_iter().collect::<HashSet<_>>().len(), 1);
    }

    #[test]
    fn row_key_numeric_equality_does_not_round_large_integer_keys() {
        let precise_integer = RowKeyValue::SignedInteger(9_007_199_254_740_993);
        let rounded_float = RowKeyValue::Float(NumberKey::new(9_007_199_254_740_992.0));

        assert_ne!(precise_integer, rounded_float);
        assert_eq!(
            [precise_integer, rounded_float]
                .into_iter()
                .collect::<HashSet<_>>()
                .len(),
            2
        );
    }

    #[test]
    fn row_key_numeric_ordering_keeps_nan_consistent_with_float_total_cmp() {
        let ordered = [
            RowKeyValue::Float(NumberKey::new(f64::from_bits(0xfff8_0000_0000_0000))),
            RowKeyValue::Float(NumberKey::new(f64::NEG_INFINITY)),
            RowKeyValue::SignedInteger(-1),
            RowKeyValue::Float(NumberKey::new(-0.5)),
            RowKeyValue::SignedInteger(0),
            RowKeyValue::UnsignedInteger(1),
            RowKeyValue::Float(NumberKey::new(1.5)),
            RowKeyValue::Float(NumberKey::new(f64::INFINITY)),
            RowKeyValue::Float(NumberKey::new(f64::NAN)),
        ];

        for pair in ordered.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{:?} should sort before {:?}",
                pair[0],
                pair[1]
            );
        }
        assert_eq!(ordered.into_iter().collect::<BTreeSet<_>>().len(), 9);
    }

    proptest! {
        #[test]
        fn row_key_eq_hash_and_cmp_equal_stay_consistent(
            left in row_key_value_strategy(),
            right in row_key_value_strategy(),
        ) {
            prop_assert_eq!(left == right, left.cmp(&right) == Ordering::Equal);
            if left == right {
                prop_assert_eq!(hash_value(&left), hash_value(&right));
            }
        }

        #[test]
        fn row_key_ordering_is_transitive(
            a in row_key_value_strategy(),
            b in row_key_value_strategy(),
            c in row_key_value_strategy(),
        ) {
            if a <= b && b <= c {
                prop_assert!(a <= c);
            }
            if a >= b && b >= c {
                prop_assert!(a >= c);
            }
        }
    }
}
