use std::collections::{BTreeMap, BTreeSet};

use core_data::{dataset_column_values, metadata_rows_dataset, LoadedDataset};
use core_engine::RuleValidationResult;
use core_rule_model::{ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity, ValueExpr};
use serde_json::Value;

use crate::dataset_helpers::dataset_column_name;
use crate::json_values::json_scalar_string;
use crate::match_datasets::SOURCE_ROW_COLUMN;
use crate::scope_filter::scope_values;
use crate::{dataset_matches_name, join_skipped_result};

type JoinResult<T> = Result<T, RuleValidationResult>;
type StudySubject = (String, String);
type Relationship = (StudySubject, String);
type RecordKey = (StudySubject, i128);

pub(crate) fn uses_pc_relrec_match(rule: &ExecutableRule) -> bool {
    let Some([declaration]) = rule.datasets.as_deref() else {
        return false;
    };
    let ConditionGroup::All(checks) = &rule.conditions else {
        return false;
    };
    let [a, b, c, d] = checks.as_slice() else {
        return false;
    };
    rule.rule_type == RuleType::RecordData
        && rule.sensitivity == Some(Sensitivity::Record)
        && rule.operations.is_empty()
        && rule.entities.is_none()
        && scope_values(rule.domains.as_ref(), "Include") == ["PP"]
        && declaration.fields.len() == 2
        && declaration.fields.get("Name").and_then(Value::as_str) == Some("RELREC")
        && declaration.fields.get("Wildcard").and_then(Value::as_str) == Some("PC")
        && rule.output_variables == ["PPCAT", "PPRFTDTC", "RELREC.PCRFTDTC", "RELREC.PCTEST"]
        && [
            (a, "PPCAT", Operator::IsNotEmpty, None),
            (b, "RELREC.PCRFTDTC", Operator::IsNotEmpty, None),
            (c, "PPRFTDTC", Operator::EqualTo, Some("RELREC.PCRFTDTC")),
            (d, "PPCAT", Operator::NotEqualTo, Some("RELREC.PCTEST")),
        ]
        .into_iter()
        .all(|(group, target, operator, other)| {
            matches!(group, ConditionGroup::Leaf(check) if
                check.operator == operator && check.target.as_deref() == Some(target)
                && check.options.extra.is_empty()
            && match (&check.comparator, other) {
                (ValueExpr::Null, None) => true,
                (ValueExpr::ColumnRef(column), Some(other)) => column == other,
                _ => false,
            })
        })
}

pub(crate) fn join_pc_through_relrec(
    rule: &ExecutableRule,
    scoped: &[LoadedDataset],
    datasets: &[LoadedDataset],
) -> JoinResult<Vec<LoadedDataset>> {
    let [pp] = scoped else {
        return Err(join_skipped_result(
            rule,
            "one in-scope PP dataset is required",
        ));
    };
    let single = |name| {
        let mut matches = datasets
            .iter()
            .filter(|dataset| dataset_matches_name(dataset, name));
        match (matches.next(), matches.next()) {
            (Some(dataset), None) => Ok(dataset),
            _ => Err(join_skipped_result(
                rule,
                format!("one {name} dataset is required"),
            )),
        }
    };
    let relrec = single("RELREC")?;
    let pc = single("PC")?;
    let [study, subject, domain, link, idvar, idval] = columns(
        rule,
        relrec,
        [
            "STUDYID", "USUBJID", "RDOMAIN", "RELID", "IDVAR", "IDVARVAL",
        ],
    )?;

    let mut left = Vec::new();
    let mut right = BTreeMap::<Relationship, Vec<i128>>::new();
    let mut other = BTreeSet::new();
    for row in 0..domain.len() {
        let kind = domain[row].as_str().unwrap_or_default().trim();
        let relationship = identity(&study[row], &subject[row]).zip(text(&link[row]));
        if !matches!(kind, "PP" | "PC") {
            other.extend(relationship);
            continue;
        }
        let relationship = relationship.ok_or_else(|| {
            join_skipped_result(rule, "RELREC endpoint has no study, subject or RELID")
        })?;
        let sequence = seq(&idval[row])
            .filter(|_| {
                idvar[row]
                    .as_str()
                    .is_some_and(|name| name.trim() == format!("{kind}SEQ"))
            })
            .ok_or_else(|| {
                join_skipped_result(
                    rule,
                    "PP/PC RELREC endpoints require explicit nonzero sequence keys",
                )
            })?;
        if kind == "PP" {
            left.push((relationship, sequence));
        } else {
            right.entry(relationship).or_default().push(sequence);
        }
    }

    let [pp_study, pp_subject, pp_seq, category, pp_date] = columns(
        rule,
        pp,
        ["STUDYID", "USUBJID", "PPSEQ", "PPCAT", "PPRFTDTC"],
    )?;
    let [pc_study, pc_subject, pc_seq, test, pc_date] = columns(
        rule,
        pc,
        ["STUDYID", "USUBJID", "PCSEQ", "PCTEST", "PCRFTDTC"],
    )?;
    let pp_index = record_index(&pp_study, &pp_subject, &pp_seq);
    let pc_index = record_index(&pc_study, &pc_subject, &pc_seq);
    let mut rows = Vec::new();
    for (relationship, pp_key) in left {
        if other.contains(&relationship) {
            return Err(join_skipped_result(
                rule,
                "PP relationship references a domain other than PC",
            ));
        }
        let Some(pc_endpoints) = right.get(&relationship) else {
            continue;
        };
        let identity = relationship.0;
        let Some(pp_rows) = pp_index.get(&(identity.clone(), pp_key)) else {
            continue;
        };
        for &pc_key in pc_endpoints {
            let Some(pc_rows) = pc_index.get(&(identity.clone(), pc_key)) else {
                continue;
            };
            for &a in pp_rows {
                for &b in pc_rows {
                    rows.push(BTreeMap::from([
                        ("USUBJID".to_owned(), pp_subject[a].clone()),
                        ("PPSEQ".to_owned(), pp_seq[a].clone()),
                        ("PPCAT".to_owned(), category[a].clone()),
                        ("PPRFTDTC".to_owned(), pp_date[a].clone()),
                        ("RELREC.PCRFTDTC".to_owned(), pc_date[b].clone()),
                        ("RELREC.PCTEST".to_owned(), test[b].clone()),
                        (SOURCE_ROW_COLUMN.to_owned(), Value::from(a + 1)),
                    ]));
                }
            }
        }
    }
    if rows.is_empty() {
        return Err(join_skipped_result(
            rule,
            "no applicable PP/PC relationship was found",
        ));
    }
    metadata_rows_dataset(pp, &rows)
        .map(|dataset| vec![dataset])
        .map_err(|error| join_skipped_result(rule, error.to_string()))
}

fn columns<const N: usize>(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
    names: [&str; N],
) -> JoinResult<[Vec<Value>; N]> {
    let mut values = [(); N].map(|()| Vec::new());
    for (output, name) in values.iter_mut().zip(names) {
        let column = dataset_column_name(dataset, name).ok_or_else(|| {
            join_skipped_result(
                rule,
                format!("dataset {} has no {name}", dataset.metadata.name),
            )
        })?;
        *output = dataset_column_values(dataset, &column)
            .map_err(|error| join_skipped_result(rule, error.to_string()))?;
    }
    Ok(values)
}

fn record_index(
    studies: &[Value],
    subjects: &[Value],
    sequences: &[Value],
) -> BTreeMap<RecordKey, Vec<usize>> {
    let mut index = BTreeMap::<RecordKey, Vec<usize>>::new();
    for (row, sequence) in sequences.iter().enumerate() {
        if let Some(key) = identity(&studies[row], &subjects[row]).zip(seq(sequence)) {
            index.entry(key).or_default().push(row);
        }
    }
    index
}

fn identity(study: &Value, subject: &Value) -> Option<StudySubject> {
    Some((text(study)?, text(subject)?))
}

fn text(value: &Value) -> Option<String> {
    let value = json_scalar_string(value)?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn seq(value: &Value) -> Option<i128> {
    let raw = text(value)?;
    let integer = match raw.split_once('.') {
        Some((integer, fraction))
            if !fraction.is_empty() && fraction.bytes().all(|byte| byte == b'0') =>
        {
            integer
        }
        Some(_) => return None,
        None => &raw,
    };
    integer.parse::<i128>().ok().filter(|value| *value != 0)
}
