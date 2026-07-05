use std::cmp::Ordering;

use core_data::LoadedDataset;
use core_rule_model::ValueExpr;
use polars::prelude::{Column, DataFrame};

use crate::scalar_operators::{expand_domain_placeholder, ScalarValue};
use crate::{
    cell_string, compare_scalars, compare_sort_values, is_uncertain_sort_value, option_string,
    optional_column, BooleanMask, Result, SortSpec,
};

pub(super) fn evaluate_target_is_not_sorted_by(
    dataset: &LoadedDataset,
    frame: &DataFrame,
    row_count: usize,
    target_column: &Column,
    comparator: &ValueExpr,
    options: &core_rule_model::OperatorOptions,
) -> Result<BooleanMask> {
    let sort_specs = crate::sort_specs(comparator)?;
    let within = option_string(&options.extra, "within")
        .map(|value| expand_domain_placeholder(dataset, &value));
    let mut groups: std::collections::BTreeMap<String, Vec<SortRow>> =
        std::collections::BTreeMap::new();

    for row in 0..row_count {
        let group_key = match within.as_deref() {
            Some(column_name) => cell_string(frame, column_name, row)?.unwrap_or_default(),
            None => String::new(),
        };
        let target = ScalarValue::from_any_value(target_column.get(row)?);
        let sort_values = sort_specs
            .iter()
            .map(|spec| {
                let column_name = expand_domain_placeholder(dataset, &spec.column);
                let Some(column) = optional_column(frame, &column_name)? else {
                    return Ok(None);
                };
                match ScalarValue::from_any_value(column.get(row)?) {
                    ScalarValue::Null => Ok(None),
                    value => Ok(Some(value)),
                }
            })
            .collect::<Result<Vec<_>>>()?;

        groups.entry(group_key).or_default().push(SortRow {
            row,
            target,
            sort_values,
        });
    }

    let mut mask = vec![false; row_count];
    for rows in groups.values() {
        let group_has_uncertain_sort = rows
            .iter()
            .any(|row| row.sort_values.iter().any(is_uncertain_sort_value));
        let group_has_inversion = mark_target_sort_inversions(rows, &sort_specs, &mut mask);

        if group_has_inversion && group_has_uncertain_sort {
            for row in rows {
                if !matches!(row.target, ScalarValue::Null) {
                    mask[row.row] = true;
                }
            }
        }
    }

    Ok(mask)
}

#[derive(Debug)]
pub(super) struct SortRow {
    pub(super) row: usize,
    pub(super) target: ScalarValue,
    pub(super) sort_values: Vec<Option<ScalarValue>>,
}

pub(super) fn mark_target_sort_inversions(
    rows: &[SortRow],
    sort_specs: &[SortSpec],
    mask: &mut [bool],
) -> bool {
    let mut sorted = rows.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| {
        compare_sort_values(&left.sort_values, &right.sort_values, sort_specs)
            .unwrap_or(Ordering::Equal)
            .then_with(|| left.row.cmp(&right.row))
    });

    if target_sort_requires_pairwise_comparison(&sorted, sort_specs) {
        return mark_target_sort_inversions_pairwise(&sorted, sort_specs, mask);
    }

    if sort_specs.len() == 1 {
        if single_sort_spec_needs_lane_split(&sorted) {
            return mark_single_sort_lane_inversions(&sorted, sort_specs, mask);
        }
    } else if target_sort_has_uncomparable_pair(&sorted, sort_specs) {
        return mark_target_sort_inversions_pairwise(&sorted, sort_specs, mask);
    }

    mark_target_sort_inversions_ranked(&sorted, sort_specs, mask)
}

fn mark_target_sort_inversions_ranked(
    sorted: &[&SortRow],
    sort_specs: &[SortSpec],
    mask: &mut [bool],
) -> bool {
    let buckets = sorted
        .chunk_by(|left, right| {
            compare_sort_values(&left.sort_values, &right.sort_values, sort_specs)
                == Some(Ordering::Equal)
        })
        .collect::<Vec<_>>();
    mark_target_sort_inversions_for_buckets(&buckets, mask)
}

fn mark_target_sort_inversions_for_buckets(buckets: &[&[&SortRow]], mask: &mut [bool]) -> bool {
    let mut numeric = Vec::new();
    let mut string = Vec::new();
    for (bucket, rows) in buckets.iter().enumerate() {
        for row in rows.iter().copied() {
            match comparable_target_sort_value(&row.target) {
                Some(ComparableTargetSortValue::Number(value)) => {
                    numeric.push(TargetSortEntry {
                        bucket,
                        row: row.row,
                        value,
                    });
                }
                Some(ComparableTargetSortValue::String(value)) => {
                    string.push(TargetSortEntry {
                        bucket,
                        row: row.row,
                        value,
                    });
                }
                None => {}
            }
        }
    }
    mark_numeric_target_sort_inversions(&numeric, mask)
        | mark_string_target_sort_inversions(&string, mask)
}

fn target_sort_requires_pairwise_comparison(rows: &[&SortRow], sort_specs: &[SortSpec]) -> bool {
    if sort_specs.len() > 1 && target_sort_has_uncomparable_pair(rows, sort_specs) {
        return true;
    }

    // Preserve compare_scalars() semantics for string columns that mix numeric-like
    // and non-numeric values: "10" vs "B" falls back to string comparison.
    target_sort_has_mixed_string_targets(rows)
}

fn target_sort_has_uncomparable_pair(rows: &[&SortRow], sort_specs: &[SortSpec]) -> bool {
    rows.iter().enumerate().any(|(left_index, left)| {
        rows[left_index + 1..].iter().any(|right| {
            compare_sort_values(&left.sort_values, &right.sort_values, sort_specs).is_none()
        })
    })
}

fn target_sort_has_mixed_string_targets(rows: &[&SortRow]) -> bool {
    let has_numeric_like_string = rows.iter().any(|row| {
        matches!(&row.target, ScalarValue::String(_))
            && row.target.as_type_insensitive_number().is_some()
    });
    let has_non_numeric_string = rows.iter().any(|row| {
        matches!(&row.target, ScalarValue::String(_))
            && row.target.as_type_insensitive_number().is_none()
    });
    has_numeric_like_string && has_non_numeric_string
}

fn single_sort_spec_needs_lane_split(rows: &[&SortRow]) -> bool {
    let mut non_null_count = 0usize;
    let mut other_count = 0usize;
    let mut has_number = false;
    let mut has_non_numeric_string = false;

    for row in rows {
        let Some(value) = row.sort_values.first().and_then(Option::as_ref) else {
            continue;
        };
        non_null_count += 1;
        match value {
            ScalarValue::Number(value) if value.is_finite() => {
                has_number = true;
            }
            ScalarValue::String(value) => {
                if value.trim().parse::<f64>().ok().is_some_and(f64::is_finite) {
                    // Numeric-like strings compare with numeric values and also
                    // bridge into the string lane when paired with non-numeric
                    // strings, so they do not make a lane split necessary by
                    // themselves.
                } else {
                    has_non_numeric_string = true;
                }
            }
            _ => {
                other_count += 1;
            }
        }
    }

    (has_number && has_non_numeric_string) || (other_count > 0 && non_null_count > 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SortLane {
    Numeric,
    String,
    Other(usize),
}

fn mark_single_sort_lane_inversions(
    sorted: &[&SortRow],
    sort_specs: &[SortSpec],
    mask: &mut [bool],
) -> bool {
    let Some(sort_spec) = sort_specs.first() else {
        return false;
    };
    // A single sort column can have overlapping comparable domains:
    // numeric values compare with numeric-like strings, numeric-like strings
    // also compare lexically with non-numeric strings, and nulls compare with
    // every lane through the null ordering policy. Split the optimized scan
    // into those lanes to preserve pairwise semantics without forcing the
    // whole group through O(n^2).
    let mut lanes: std::collections::BTreeMap<SortLane, Vec<&SortRow>> =
        std::collections::BTreeMap::new();
    let mut null_rows = Vec::new();

    for row in sorted {
        match row.sort_values.first().and_then(Option::as_ref) {
            None => null_rows.push(*row),
            Some(value) => {
                let mut assigned = false;
                if value.as_type_insensitive_number().is_some() {
                    lanes.entry(SortLane::Numeric).or_default().push(*row);
                    assigned = true;
                }
                if value.as_string().is_some() {
                    lanes.entry(SortLane::String).or_default().push(*row);
                    assigned = true;
                }
                if !assigned {
                    // Values that compare to nulls but not to each other stay in
                    // one-row lanes, which preserves pairwise semantics without
                    // forcing the whole group through O(n^2).
                    lanes
                        .entry(SortLane::Other(row.row))
                        .or_default()
                        .push(*row);
                }
            }
        }
    }

    if lanes.is_empty() {
        return false;
    }

    let mut has_inversion = false;
    for lane_rows in lanes.values_mut() {
        lane_rows.extend(null_rows.iter().copied());
        lane_rows.sort_by(|left, right| {
            compare_sort_values(&left.sort_values, &right.sort_values, sort_specs)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.row.cmp(&right.row))
        });
        if target_sort_has_mixed_string_targets(lane_rows) {
            has_inversion |= mark_target_sort_inversions_pairwise(lane_rows, sort_specs, mask);
        } else {
            has_inversion |= mark_target_sort_inversions_ranked(
                lane_rows,
                std::slice::from_ref(sort_spec),
                mask,
            );
        }
    }

    has_inversion
}

pub(super) fn mark_target_sort_inversions_pairwise(
    sorted: &[&SortRow],
    sort_specs: &[SortSpec],
    mask: &mut [bool],
) -> bool {
    let mut has_inversion = false;
    for (left_index, left) in sorted.iter().enumerate() {
        for right in &sorted[left_index + 1..] {
            let Some(sort_ordering) =
                compare_sort_values(&left.sort_values, &right.sort_values, sort_specs)
            else {
                continue;
            };
            if sort_ordering == Ordering::Equal {
                continue;
            }
            let Some(target_ordering) = compare_scalars(&left.target, &right.target) else {
                continue;
            };
            if target_ordering != Ordering::Equal && sort_ordering != target_ordering {
                mask[left.row] = true;
                mask[right.row] = true;
                has_inversion = true;
            }
        }
    }
    has_inversion
}

#[derive(Debug, Clone)]
enum ComparableTargetSortValue {
    Number(f64),
    String(String),
}

#[derive(Debug)]
struct TargetSortEntry<T> {
    bucket: usize,
    row: usize,
    value: T,
}

fn comparable_target_sort_value(value: &ScalarValue) -> Option<ComparableTargetSortValue> {
    if let Some(value) = value.as_type_insensitive_number() {
        return Some(ComparableTargetSortValue::Number(normalize_zero(value)));
    }
    value
        .as_string()
        .map(|value| ComparableTargetSortValue::String(value.to_owned()))
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

fn mark_numeric_target_sort_inversions(
    entries: &[TargetSortEntry<f64>],
    mask: &mut [bool],
) -> bool {
    let mut values = entries.iter().map(|entry| entry.value).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|left, right| *left == *right);
    mark_target_sort_inversions_by_rank(
        entries,
        mask,
        |entry| {
            values
                .binary_search_by(|value| value.total_cmp(&entry.value))
                .expect("target value rank exists")
        },
        values.len(),
    )
}

fn mark_string_target_sort_inversions(
    entries: &[TargetSortEntry<String>],
    mask: &mut [bool],
) -> bool {
    let values = entries
        .iter()
        .map(|entry| entry.value.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let ranks = values
        .into_iter()
        .enumerate()
        .map(|(rank, value)| (value, rank))
        .collect::<std::collections::BTreeMap<_, _>>();
    mark_target_sort_inversions_by_rank(
        entries,
        mask,
        |entry| *ranks.get(&entry.value).expect("target value rank exists"),
        ranks.len(),
    )
}

fn mark_target_sort_inversions_by_rank<T>(
    entries: &[TargetSortEntry<T>],
    mask: &mut [bool],
    rank: impl Fn(&TargetSortEntry<T>) -> usize,
    rank_count: usize,
) -> bool {
    if entries.len() < 2 || rank_count < 2 {
        return false;
    }

    let mut has_inversion = false;
    let mut seen = FenwickCounts::new(rank_count);
    for bucket in entries.chunk_by(|left, right| left.bucket == right.bucket) {
        for entry in bucket {
            if seen.has_greater_than(rank(entry)) {
                mask[entry.row] = true;
                has_inversion = true;
            }
        }
        for entry in bucket {
            seen.add(rank(entry));
        }
    }

    let mut seen = FenwickCounts::new(rank_count);
    for bucket in entries
        .chunk_by(|left, right| left.bucket == right.bucket)
        .rev()
    {
        for entry in bucket {
            if seen.has_less_than(rank(entry)) {
                mask[entry.row] = true;
                has_inversion = true;
            }
        }
        for entry in bucket {
            seen.add(rank(entry));
        }
    }

    has_inversion
}

#[derive(Debug)]
struct FenwickCounts {
    counts: Vec<usize>,
    total: usize,
}

impl FenwickCounts {
    fn new(len: usize) -> Self {
        Self {
            counts: vec![0; len + 1],
            total: 0,
        }
    }

    fn add(&mut self, rank: usize) {
        self.total += 1;
        let mut index = rank + 1;
        while index < self.counts.len() {
            self.counts[index] += 1;
            index += fenwick_lowbit(index);
        }
    }

    fn has_less_than(&self, rank: usize) -> bool {
        rank > 0 && self.prefix_sum(rank - 1) > 0
    }

    fn has_greater_than(&self, rank: usize) -> bool {
        self.total > self.prefix_sum(rank)
    }

    fn prefix_sum(&self, rank: usize) -> usize {
        let mut index = rank + 1;
        let mut total = 0;
        while index > 0 {
            total += self.counts[index];
            index &= index - 1;
        }
        total
    }
}

fn fenwick_lowbit(index: usize) -> usize {
    index & index.wrapping_neg()
}
