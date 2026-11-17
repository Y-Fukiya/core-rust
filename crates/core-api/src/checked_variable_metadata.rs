use std::collections::{BTreeMap, BTreeSet};

use core_data::{metadata_row_dataset, LoadedDataset};
use core_engine::RuleValidationResult;
use core_rule_model::{
    ConditionGroup, ExecutableRule, OperationSpec, Operator, RuleType, Sensitivity, ValueExpr,
};
use serde_json::Value;

use crate::metadata_execution::insert_metadata_operation_value;
use crate::operation_fields::{
    clean_operation_identifier, operation_name, operation_value, string_field,
};
use crate::{
    dataset_metadata_name, filter_datasets_by_rule_scope, operation_skipped_result, ValidateRequest,
};

const SDTMIG_VARIABLES: &str = include_str!("checked_sdtmig_variables.tsv");

#[derive(Clone, Copy)]
enum ListOperation {
    MissingExpected,
    LibraryIntersection,
}

pub(crate) struct VariableListPlan {
    operation: ListOperation,
    library_output: String,
    dataset_output: String,
    result_output: String,
}

impl VariableListPlan {
    pub(crate) fn from_rule(rule: &ExecutableRule) -> Option<Self> {
        if rule.rule_type != RuleType::VariableMetadata
            || rule.sensitivity != Some(Sensitivity::Dataset)
        {
            return None;
        }
        let [library, dataset, combined] = rule.operations.as_slice() else {
            return None;
        };
        let operation = match (
            operation_name(library)?.as_str(),
            operation_name(combined)?.as_str(),
        ) {
            ("expected_variables", "minus") if uses_default(combined, "order_insensitive") => {
                ListOperation::MissingExpected
            }
            ("get_column_order_from_library", "intersect")
                if uses_default(combined, "case_sensitive") =>
            {
                ListOperation::LibraryIntersection
            }
            _ => return None,
        };
        if operation_name(dataset)?.as_str() != "get_column_order_from_dataset"
            || ["key_name", "key_value", "filter", "domain"]
                .iter()
                .any(|field| operation_value(library, &[*field]).is_some())
        {
            return None;
        }
        let library_output = output_id(library)?;
        let dataset_output = output_id(dataset)?;
        let result_output = output_id(combined)?;
        let names = [&library_output, &dataset_output, &result_output]
            .into_iter()
            .map(|id| clean_operation_identifier(id).to_ascii_uppercase())
            .collect::<BTreeSet<_>>();
        if names.len() != 3
            || names.contains("DATASET_NAME")
            || names.contains("VARIABLE_NAME")
            || string_field(combined, &["name"]).as_deref() != Some(&library_output)
            || string_field(combined, &["subtract"]).as_deref() != Some(&dataset_output)
        {
            return None;
        }
        let check = match &rule.conditions {
            ConditionGroup::All(checks) => {
                let [ConditionGroup::Leaf(check)] = checks.as_slice() else {
                    return None;
                };
                check
            }
            ConditionGroup::Leaf(check) => check,
            _ => return None,
        };
        let (operator, target, comparator) = match operation {
            ListOperation::MissingExpected => (
                Operator::NotContainsAll,
                "variable_name".to_owned(),
                &library_output,
            ),
            ListOperation::LibraryIntersection => (
                Operator::IsNotOrderedSubsetOf,
                clean_operation_identifier(&dataset_output),
                &result_output,
            ),
        };
        if check.operator != operator
            || check
                .target
                .as_deref()
                .map(clean_operation_identifier)
                .as_deref()
                != Some(&target)
            || !matches!(&check.comparator, ValueExpr::ColumnRef(name) if name == &clean_operation_identifier(comparator))
        {
            return None;
        }
        Some(Self {
            operation,
            library_output,
            dataset_output,
            result_output,
        })
    }

    pub(crate) fn execute(
        &self,
        rule: &ExecutableRule,
        datasets: &[LoadedDataset],
        request: &ValidateRequest,
    ) -> Result<Vec<LoadedDataset>, RuleValidationResult> {
        let version = request
            .standard_version
            .as_deref()
            .filter(|_| {
                request
                    .standard
                    .as_deref()
                    .is_some_and(|name| name.eq_ignore_ascii_case("SDTMIG"))
            })
            .map(|version| version.replace('-', "."))
            .ok_or_else(|| {
                operation_skipped_result(rule, "explicit SDTMIG standard and version are required")
            })?;
        let scoped = filter_datasets_by_rule_scope(rule, datasets);
        if scoped.is_empty() {
            return Err(operation_skipped_result(
                rule,
                "no dataset is in rule scope",
            ));
        }
        scoped
            .iter()
            .map(|dataset| {
                let kind = match self.operation {
                    ListOperation::MissingExpected => "expected",
                    ListOperation::LibraryIntersection => "order",
                };
                let name = dataset.metadata.name.to_ascii_uppercase();
                let domain = if name.starts_with("SUPP") && name.len() > 4 {
                    "SUPPQUAL"
                } else {
                    &name
                };
                let library = library_variables(&version, domain, kind).ok_or_else(|| {
                    operation_skipped_result(
                        rule,
                        format!("no reviewed SDTMIG {version} {kind} metadata for {name}"),
                    )
                })?;
                let columns = dataset.summary().columns;
                let present = columns.iter().map(String::as_str).collect::<BTreeSet<_>>();
                let selected = library
                    .split('|')
                    .filter(|name| {
                        present.contains(name)
                            == matches!(self.operation, ListOperation::LibraryIntersection)
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                let physical_order = columns.join("|");
                let mut row = BTreeMap::from([
                    (
                        "dataset_name".to_owned(),
                        Value::String(dataset_metadata_name(dataset)),
                    ),
                    (
                        "variable_name".to_owned(),
                        Value::String(physical_order.clone()),
                    ),
                ]);
                insert_metadata_operation_value(
                    &mut row,
                    &self.library_output,
                    Value::String(library.to_owned()),
                );
                insert_metadata_operation_value(
                    &mut row,
                    &self.dataset_output,
                    Value::String(physical_order),
                );
                insert_metadata_operation_value(
                    &mut row,
                    &self.result_output,
                    Value::String(selected),
                );
                metadata_row_dataset(dataset, &row)
                    .map_err(|error| operation_skipped_result(rule, error.to_string()))
            })
            .collect()
    }
}

fn library_variables(version: &str, domain: &str, kind: &str) -> Option<&'static str> {
    SDTMIG_VARIABLES.lines().skip(1).find_map(|row| {
        let mut fields = row.split('\t');
        let key = (fields.next()?, fields.next()?, fields.next()?);
        let values = fields.next()?;
        (key == (version, domain, kind)).then_some(values)
    })
}

fn uses_default(operation: &OperationSpec, field: &str) -> bool {
    operation_value(operation, &[field]).is_none_or(|value| value.as_bool() == Some(true))
}

fn output_id(operation: &OperationSpec) -> Option<String> {
    string_field(operation, &["id"]).filter(|name| {
        name.strip_prefix('$').is_some_and(|id| {
            !id.is_empty()
                && id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        })
    })
}
