use core_data::LoadedDataset;
use core_rule_model::{ExecutableRule, OperationSpec};
use serde_json::Value;

use crate::{
    dataset_helpers::dataset_domain_value, engine_semantics, operation_fields::operation_name,
};

const MODEL_DOMAIN_LABELS: &str = "model_domain_labels";

pub(crate) fn bind_standard_domain_labels(
    rule: &mut ExecutableRule,
    standard: Option<&str>,
    version: Option<&str>,
) {
    let labels = standard
        .zip(version)
        .filter(|_| engine_semantics::is_requested_standard_operation_rule(rule))
        .map(|(standard, version)| verified_domain_labels(standard, version))
        .unwrap_or_default();
    let binding = (!labels.is_empty()).then(|| {
        Value::Object(
            labels
                .iter()
                .map(|&(domain, label)| (domain.to_owned(), Value::String(label.to_owned())))
                .collect(),
        )
    });
    for operation in &mut rule.operations {
        if operation_name(operation).as_deref() == Some("domain_label") {
            if let Some(binding) = &binding {
                operation
                    .fields
                    .insert(MODEL_DOMAIN_LABELS.to_owned(), binding.clone());
            } else {
                operation.fields.remove(MODEL_DOMAIN_LABELS);
            }
        }
    }
}

fn verified_domain_labels(
    standard: &str,
    version: &str,
) -> &'static [(&'static str, &'static str)] {
    match (standard.to_ascii_uppercase().as_str(), version) {
        ("SENDIG", "3.0" | "3.1" | "3.1.1") => &[("LB", "Laboratory")],
        ("SDTMIG", "3.2") => &[("LB", "Labs"), ("CM", "Concomitant Medications")],
        ("SDTMIG", "3.3" | "3.4") => &[
            ("LB", "Laboratory Test Results"),
            ("CM", "Concomitant/Prior Medications"),
        ],
        _ => &[],
    }
}

pub(crate) fn bound_domain_label<'a>(
    operation: &'a OperationSpec,
    dataset: &LoadedDataset,
) -> Option<&'a str> {
    operation
        .fields
        .get(MODEL_DOMAIN_LABELS)?
        .get(dataset_domain_value(dataset))?
        .as_str()
}
