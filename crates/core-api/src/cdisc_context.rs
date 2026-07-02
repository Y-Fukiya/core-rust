use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use core_cdisc_library::{
    load_ct_json_file, load_define_xml_file, load_external_dictionary_file, ControlledTerminology,
    DefineXmlMetadata,
};
use core_rule_model::{Condition, ConditionGroup, Operator, ValueExpr};
use serde_json::Value;

use crate::Result;

#[derive(Debug, Clone, Default)]
pub(crate) struct CdiscContext {
    define_xml: Vec<DefineXmlMetadata>,
    terminology: ControlledTerminology,
}

impl CdiscContext {
    pub(crate) fn load(
        define_xml_paths: &[PathBuf],
        ct_paths: &[PathBuf],
        external_dictionary_paths: &[PathBuf],
    ) -> Result<Self> {
        let define_xml = define_xml_paths
            .iter()
            .map(load_define_xml_file)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut terminology = ControlledTerminology::default();

        for define in &define_xml {
            for (canonical, aliases) in &define.codelist_aliases {
                for alias in aliases {
                    terminology.insert_alias(canonical, alias);
                }
            }
            for term in &define.codelists {
                terminology.insert_term(&term.codelist, term.value.clone());
            }
        }

        for path in ct_paths {
            let ct = load_ct_json_file(path)?;
            merge_terminology(&mut terminology, ct);
        }

        for path in external_dictionary_paths {
            let dictionary = load_external_dictionary_file(path)?;
            merge_terminology(&mut terminology, dictionary);
        }

        Ok(Self {
            define_xml,
            terminology,
        })
    }
}

fn merge_terminology(target: &mut ControlledTerminology, source: ControlledTerminology) {
    for (alias, canonical) in source.aliases {
        target.insert_alias(canonical, alias);
    }
    for (codelist, values) in source.codelists {
        for value in values {
            target.insert_term(&codelist, value);
        }
    }
}

pub(crate) fn apply_cdisc_context_to_group(group: &mut ConditionGroup, context: &CdiscContext) {
    match group {
        ConditionGroup::All(groups) | ConditionGroup::Any(groups) => {
            for group in groups {
                apply_cdisc_context_to_group(group, context);
            }
        }
        ConditionGroup::Not(group) => apply_cdisc_context_to_group(group, context),
        ConditionGroup::Leaf(condition) => apply_cdisc_context_to_condition(condition, context),
    }
}

fn apply_cdisc_context_to_condition(condition: &mut Condition, context: &CdiscContext) {
    if !matches!(
        condition.operator,
        Operator::IsContainedBy
            | Operator::IsNotContainedBy
            | Operator::IsContainedByCaseInsensitive
            | Operator::IsNotContainedByCaseInsensitive
    ) || !matches!(condition.comparator, ValueExpr::Null)
    {
        return;
    }

    let Some(codelist) =
        condition_codelist(condition).or_else(|| define_codelist_for_condition(condition, context))
    else {
        return;
    };

    let Some(values) = context.terminology.values(&codelist) else {
        return;
    };

    condition.comparator = ValueExpr::List(
        values
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    );
}

fn condition_codelist(condition: &Condition) -> Option<String> {
    option_string_field(
        &condition.options.extra,
        &[
            "codelist",
            "codelist_oid",
            "ct_codelist",
            "define_codelist",
            "dictionary",
            "dictionary_name",
            "dictionary_id",
            "external_dictionary",
            "external_dictionary_name",
            "CodeListOID",
            "CodeList",
        ],
    )
}

pub(crate) fn define_codelist_for_condition(
    condition: &Condition,
    context: &CdiscContext,
) -> Option<String> {
    let target = condition.target.as_deref()?;
    let target_candidates = target_name_candidates(target);
    if let Some((domain, _unqualified)) = target.rsplit_once('.') {
        let domain_matches = context
            .define_xml
            .iter()
            .flat_map(|define| {
                define
                    .datasets
                    .iter()
                    .filter(move |dataset| {
                        dataset
                            .domain
                            .as_deref()
                            .or(dataset.name.as_deref())
                            .is_some_and(|name| name.eq_ignore_ascii_case(domain))
                    })
                    .flat_map(|dataset| {
                        dataset.item_refs.iter().filter_map(|item_ref| {
                            let item_oid = item_ref.item_oid.as_deref()?;
                            define
                                .variables
                                .iter()
                                .find(|variable| {
                                    variable.oid.as_deref() == Some(item_oid)
                                        && target_candidates.iter().any(|target| {
                                            variable.name.eq_ignore_ascii_case(target)
                                        })
                                })
                                .and_then(|variable| variable.codelist_oid.clone())
                        })
                    })
            })
            .collect::<Vec<_>>();
        if let Some(codelist) = unique_codelist(domain_matches) {
            return Some(codelist);
        }
    }

    let global_matches = context
        .define_xml
        .iter()
        .flat_map(|define| &define.variables)
        .filter(|variable| {
            target_candidates
                .iter()
                .any(|target| variable.name.eq_ignore_ascii_case(target))
        })
        .filter_map(|variable| variable.codelist_oid.clone())
        .collect::<Vec<_>>();
    unique_codelist(global_matches)
}

fn unique_codelist(codelists: Vec<String>) -> Option<String> {
    let unique = codelists.into_iter().collect::<BTreeSet<_>>();
    (unique.len() == 1)
        .then(|| unique.into_iter().next())
        .flatten()
}

fn target_name_candidates(target: &str) -> Vec<&str> {
    let mut candidates = vec![target];
    if let Some((_prefix, unqualified)) = target.rsplit_once('.') {
        candidates.push(unqualified);
    }
    candidates
}

fn option_string_field(map: &BTreeMap<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| {
            map.get(*key).or_else(|| {
                map.iter()
                    .find(|(candidate, _value)| candidate.eq_ignore_ascii_case(key))
                    .map(|(_key, value)| value)
            })
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
}
