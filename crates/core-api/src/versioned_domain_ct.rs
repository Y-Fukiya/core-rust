use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, PathBuf};

use core_data::{metadata_rows_dataset, LoadedDataset};
use core_engine::RuleValidationResult;
use core_rule_model::{
    Condition, ConditionGroup, ExecutableRule, Operator, RuleType, Sensitivity, ValueExpr,
};
use roxmltree::{Document, Node};
use serde_json::Value;

use crate::dataset_helpers::dataset_metadata_name;
use crate::match_datasets::SOURCE_ROW_COLUMN;
use crate::scope_filter::filter_datasets_by_rule_scope;
use crate::{open_rules_env_value, operation_skipped_result, DatasetLoader, ValidateRequest};

const REFERENCE: &str = include_str!("checked_domain_ct.tsv");
const OUTPUT: &str = "define_variable_codelist_coded_codes";
const INVALID: &str = "__core_invalid_domain_term_code";
const CODELIST: &str = "C66734";
type Checked<T> = Result<T, String>;

pub(crate) fn uses_versioned_domain_ct(rule: &ExecutableRule) -> bool {
    let [custom, codes] = rule.operations.as_slice() else {
        return false;
    };
    let ConditionGroup::All(checks) = &rule.conditions else {
        return false;
    };
    let [ConditionGroup::Leaf(standard), ConditionGroup::Leaf(codelist), ConditionGroup::Leaf(membership)] =
        checks.as_slice()
    else {
        return false;
    };
    rule.rule_type == RuleType::VariableMetadata
        && rule.sensitivity == Some(Sensitivity::Record)
        && rule.entities.is_none()
        && rule.datasets.as_ref().is_none_or(Vec::is_empty)
        && rule.output_variables == [OUTPUT]
        && custom.fields.len() == 2
        && custom.fields.get("operator").and_then(Value::as_str) == Some("domain_is_custom")
        && custom.fields.get("id").and_then(Value::as_str) == Some("$domain_is_custom")
        && codes.fields.len() == 5
        && [
            ("operator", "get_codelist_attributes"),
            ("id", "$domain_lib_ccode"),
            ("name", "DOMAIN"),
            ("ct_attribute", "Term CCODE"),
        ]
        .into_iter()
        .all(|(key, value)| codes.fields.get(key).and_then(Value::as_str) == Some(value))
        && codes.fields.get("ct_conditions")
            == Some(&serde_json::json!([{"Codelist Code": CODELIST}]))
        && standard.target.as_deref() == Some("$domain_is_custom")
        && standard.operator == Operator::EqualTo
        && (standard.comparator == ValueExpr::Literal(Value::Bool(false))
            || matches!(&standard.comparator, ValueExpr::ColumnRef(value) if value == "false"))
        && codelist.target.as_deref() == Some("define_variable_ccode")
        && codelist.operator == Operator::EqualTo
        && matches!(&codelist.comparator, ValueExpr::Literal(value) if value.as_str() == Some(CODELIST))
        && membership.target.as_deref() == Some(OUTPUT)
        && membership.operator == Operator::IsNotContainedBy
        && matches!(&membership.comparator, ValueExpr::ColumnRef(name) if name == "domain_lib_ccode")
        && [standard, codelist, membership]
            .iter()
            .all(|check| check.options.extra.is_empty())
}

pub(crate) fn bind_versioned_domain_ct(
    rule: &mut ExecutableRule,
    datasets: &[LoadedDataset],
    request: &ValidateRequest,
) -> Result<Vec<LoadedDataset>, RuleValidationResult> {
    let projected = execution_datasets(rule, datasets, request)
        .map_err(|error| operation_skipped_result(rule, error))?;
    rule.conditions = ConditionGroup::Leaf(Condition {
        target: Some(INVALID.to_owned()),
        operator: Operator::EqualTo,
        comparator: ValueExpr::Literal(Value::Bool(true)),
        options: Default::default(),
    });
    Ok(projected)
}

fn execution_datasets(
    rule: &ExecutableRule,
    datasets: &[LoadedDataset],
    request: &ValidateRequest,
) -> Checked<Vec<LoadedDataset>> {
    if !request
        .standard
        .as_deref()
        .is_some_and(|standard| standard.eq_ignore_ascii_case("SDTMIG"))
        || !request.ct_paths.is_empty()
    {
        return Err(
            "the checked route requires SDTMIG and Define-declared CT without overrides".to_owned(),
        );
    }
    let version = request
        .standard_version
        .as_deref()
        .unwrap_or_default()
        .replace('-', ".");
    let standard_domains = reference("SDTMIG", &version)?;
    let scoped = filter_datasets_by_rule_scope(rule, datasets);
    let path = define_path(request, &scoped)?;
    let xml =
        fs::read_to_string(&path).map_err(|error| format!("cannot read Define-XML: {error}"))?;
    let document = Document::parse(&xml).map_err(|error| format!("invalid Define-XML: {error}"))?;
    let metadata = unique(
        document
            .descendants()
            .filter(|node| tag(*node, "MetaDataVersion")),
        "Define MetaDataVersion",
    )?;
    let ct = unique(
        metadata
            .children()
            .filter(|node| tag(*node, "Standards"))
            .flat_map(|node| node.children())
            .filter(|node| {
                tag(*node, "Standard")
                    && node.attribute("Type") == Some("CT")
                    && node.attribute("PublishingSet") == Some("SDTM")
            }),
        "Define SDTM CT declaration",
    )?;
    let codes = reference("SDTMCT-C66734", ct.attribute("Version").unwrap_or_default())?
        .split('|')
        .collect::<BTreeSet<_>>();
    let items = by_oid(metadata, "ItemDef")?;
    let codelists = by_oid(metadata, "CodeList")?;
    let mut execution = Vec::new();
    for dataset in &scoped {
        let name = dataset_metadata_name(dataset);
        let domain = dataset
            .metadata
            .domain
            .as_deref()
            .unwrap_or(&name)
            .to_ascii_uppercase();
        let standard_name = if domain.starts_with("SUPP") {
            "SUPPQUAL"
        } else {
            &domain
        };
        if !standard_domains
            .split('|')
            .any(|value| value == standard_name)
        {
            continue;
        }
        let itemgroup = unique(
            metadata.children().filter(|node| {
                tag(*node, "ItemGroupDef")
                    && node
                        .attribute("Name")
                        .is_some_and(|value| value.eq_ignore_ascii_case(&name))
                    && node
                        .attribute("Domain")
                        .is_none_or(|value| value.eq_ignore_ascii_case(&domain))
            }),
            &format!("Define ItemGroup for {name}"),
        )?;
        let mut rows = Vec::new();
        for (position, itemref) in itemgroup
            .children()
            .filter(|node| tag(*node, "ItemRef"))
            .enumerate()
        {
            let oid = itemref.attribute("ItemOID").unwrap_or_default();
            let item = items
                .get(oid)
                .ok_or_else(|| format!("unresolved Define ItemOID {oid}"))?;
            let Some(codelist_ref) = item.children().find(|node| tag(*node, "CodeListRef")) else {
                continue;
            };
            let oid = codelist_ref.attribute("CodeListOID").unwrap_or_default();
            let codelist = codelists
                .get(oid)
                .ok_or_else(|| format!("unresolved Define CodeListOID {oid}"))?;
            let alias = codelist.children().find(|node| {
                tag(*node, "Alias") && node.attribute("Context") == Some("nci:ExtCodeID")
            });
            if alias.and_then(|node| node.attribute("Name")) != Some(CODELIST) {
                continue;
            }
            let aliases = codelist
                .children()
                .filter(|node| tag(*node, "CodeListItem") || tag(*node, "EnumeratedItem"))
                .flat_map(|node| node.children())
                .filter(|node| tag(*node, "Alias"))
                .filter_map(|node| node.attribute("Name"))
                .collect::<Vec<_>>();
            rows.push(BTreeMap::from([
                (OUTPUT.to_owned(), Value::String(aliases.join("|"))),
                (
                    INVALID.to_owned(),
                    Value::Bool(!aliases.iter().any(|alias| codes.contains(alias))),
                ),
                (SOURCE_ROW_COLUMN.to_owned(), Value::from(position + 1)),
            ]));
        }
        if !rows.is_empty() {
            execution
                .push(metadata_rows_dataset(dataset, &rows).map_err(|error| error.to_string())?);
        }
    }
    if execution.is_empty() {
        return Err("no standard-domain Define item references C66734".to_owned());
    }
    Ok(execution)
}

fn reference(family: &str, version: &str) -> Checked<&'static str> {
    REFERENCE
        .lines()
        .find_map(|line| {
            let mut fields = line.splitn(3, '\t');
            (fields.next()? == family && fields.next()? == version)
                .then(|| fields.next())
                .flatten()
        })
        .ok_or_else(|| format!("no checked {family} reference for {version}"))
}

fn define_path(request: &ValidateRequest, datasets: &[LoadedDataset]) -> Checked<PathBuf> {
    if datasets.is_empty() {
        return Err("no dataset is in rule scope".to_owned());
    }
    if request.dataset_loader == DatasetLoader::OpenRulesDataDir
        && datasets
            .iter()
            .any(|dataset| open_rules_env_value(dataset, "CT").is_some())
    {
        return Err("the checked route does not support an explicit CT override".to_owned());
    }
    match request.define_xml_paths.as_slice() {
        [path] => Ok(path.clone()),
        [] if request.dataset_loader == DatasetLoader::OpenRulesDataDir => {
            let first = &datasets[0];
            let dir = first
                .metadata
                .full_path
                .parent()
                .ok_or("dataset has no parent directory")?;
            if datasets
                .iter()
                .any(|dataset| dataset.metadata.full_path.parent() != Some(dir))
            {
                return Err(
                    "the checked route requires datasets from one study directory".to_owned(),
                );
            }
            let name = open_rules_env_value(first, "DEFINE_XML").ok_or("DEFINE_XML is required")?;
            let path = std::path::Path::new(&name);
            let mut parts = path.components();
            if !matches!(
                (parts.next(), parts.next()),
                (Some(Component::Normal(_)), None)
            ) {
                return Err("DEFINE_XML must be a case-local filename".to_owned());
            }
            let path = dir.join(path);
            if !fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
                return Err("DEFINE_XML must be a regular case-local file".to_owned());
            }
            Ok(path)
        }
        _ => Err("one Define-XML document is required".to_owned()),
    }
}

fn tag(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn unique<'a, 'input>(
    mut nodes: impl Iterator<Item = Node<'a, 'input>>,
    description: &str,
) -> Checked<Node<'a, 'input>> {
    match (nodes.next(), nodes.next()) {
        (Some(node), None) => Ok(node),
        _ => Err(format!("exactly one {description} is required")),
    }
}

fn by_oid<'a, 'input>(
    parent: Node<'a, 'input>,
    name: &str,
) -> Checked<BTreeMap<&'a str, Node<'a, 'input>>> {
    let mut nodes = BTreeMap::new();
    for node in parent.children().filter(|node| tag(*node, name)) {
        let oid = node
            .attribute("OID")
            .ok_or_else(|| format!("Define {name} has no OID"))?;
        if nodes.insert(oid, node).is_some() {
            return Err(format!("duplicate Define {name} OID {oid}"));
        }
    }
    Ok(nodes)
}
