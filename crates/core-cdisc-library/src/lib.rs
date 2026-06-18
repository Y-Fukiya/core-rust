#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CdiscLibraryError>;

#[derive(Debug, Error)]
pub enum CdiscLibraryError {
    #[error("failed to read file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse CT JSON {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("invalid regular expression: {0}")]
    Regex(#[from] regex::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineXmlMetadata {
    pub variables: Vec<DefineVariable>,
    pub codelists: Vec<ControlledTerm>,
    pub datasets: Vec<DefineDataset>,
    pub value_lists: Vec<DefineValueList>,
    pub where_clauses: Vec<DefineWhereClause>,
    pub methods: Vec<DefineMethod>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineVariable {
    pub oid: Option<String>,
    pub name: String,
    pub data_type: Option<String>,
    pub length: Option<String>,
    pub codelist_oid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ControlledTerm {
    pub codelist: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineDataset {
    pub oid: Option<String>,
    pub name: Option<String>,
    pub domain: Option<String>,
    pub purpose: Option<String>,
    pub repeating: Option<String>,
    pub item_refs: Vec<DefineItemRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineItemRef {
    pub item_oid: Option<String>,
    pub order_number: Option<String>,
    pub mandatory: Option<String>,
    pub method_oid: Option<String>,
    pub where_clause_oid: Option<String>,
    pub value_list_oid: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineValueList {
    pub oid: Option<String>,
    pub item_refs: Vec<DefineItemRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineWhereClause {
    pub oid: Option<String>,
    pub range_checks: Vec<DefineRangeCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineRangeCheck {
    pub item_oid: Option<String>,
    pub comparator: Option<String>,
    pub soft_hard: Option<String>,
    pub check_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineMethod {
    pub oid: Option<String>,
    pub name: Option<String>,
    pub method_type: Option<String>,
    pub formal_expressions: Vec<DefineFormalExpression>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineFormalExpression {
    pub context: Option<String>,
    pub expression: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ControlledTerminology {
    pub codelists: BTreeMap<String, BTreeSet<String>>,
}

impl ControlledTerminology {
    pub fn contains(&self, codelist: &str, value: &str) -> bool {
        self.codelists
            .get(codelist)
            .is_some_and(|values| values.contains(value))
    }
}

pub fn load_define_xml_file(path: impl AsRef<Path>) -> Result<DefineXmlMetadata> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| CdiscLibraryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    parse_define_xml(&source)
}

pub fn parse_define_xml(source: &str) -> Result<DefineXmlMetadata> {
    let item_def = Regex::new(r#"(?s)<ItemDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</ItemDef>"#)?;
    let self_closing_item_def = Regex::new(r#"<ItemDef\b(?P<attrs>[^>]*)/>"#)?;
    let item_group_def =
        Regex::new(r#"(?s)<ItemGroupDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</ItemGroupDef>"#)?;
    let value_list_def =
        Regex::new(r#"(?s)<ValueListDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</ValueListDef>"#)?;
    let where_clause_def =
        Regex::new(r#"(?s)<WhereClauseDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</WhereClauseDef>"#)?;
    let range_check =
        Regex::new(r#"(?s)<RangeCheck\b(?P<attrs>[^>]*)>(?P<body>.*?)</RangeCheck>"#)?;
    let check_value = Regex::new(r#"(?s)<CheckValue\b[^>]*>(?P<value>.*?)</CheckValue>"#)?;
    let method_def = Regex::new(r#"(?s)<MethodDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</MethodDef>"#)?;
    let formal_expression =
        Regex::new(r#"(?s)<FormalExpression\b(?P<attrs>[^>]*)>(?P<body>.*?)</FormalExpression>"#)?;
    let item_ref = Regex::new(r#"<ItemRef\b(?P<attrs>[^>]*)/?>"#)?;
    let codelist_ref = Regex::new(r#"<CodeListRef\b(?P<attrs>[^>]*)/?>"#)?;
    let codelist = Regex::new(r#"(?s)<CodeList\b(?P<attrs>[^>]*)>(?P<body>.*?)</CodeList>"#)?;
    let codelist_item = Regex::new(r#"<(?:CodeListItem|EnumeratedItem)\b(?P<attrs>[^>]*)/?>"#)?;

    let mut variables = item_def
        .captures_iter(source)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            define_variable_from_item(attrs, body, &codelist_ref)
        })
        .filter(|variable| !variable.name.is_empty())
        .collect::<Vec<_>>();
    variables.extend(
        self_closing_item_def
            .captures_iter(source)
            .map(|capture| {
                let attrs = capture
                    .name("attrs")
                    .map(|value| value.as_str())
                    .unwrap_or("");
                define_variable_from_item(attrs, "", &codelist_ref)
            })
            .filter(|variable| !variable.name.is_empty()),
    );

    let codelists = codelist
        .captures_iter(source)
        .flat_map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let codelist_id = xml_attr(attrs, "OID")
                .or_else(|| xml_attr(attrs, "Name"))
                .unwrap_or_default();
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            codelist_item.captures_iter(body).filter_map(move |item| {
                let item_attrs = item.name("attrs")?.as_str();
                Some(ControlledTerm {
                    codelist: codelist_id.clone(),
                    value: xml_attr(item_attrs, "CodedValue")?,
                })
            })
        })
        .collect();

    let datasets = item_group_def
        .captures_iter(source)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineDataset {
                oid: xml_attr(attrs, "OID"),
                name: xml_attr(attrs, "Name"),
                domain: xml_attr(attrs, "Domain"),
                purpose: xml_attr(attrs, "Purpose"),
                repeating: xml_attr(attrs, "Repeating"),
                item_refs: parse_item_refs(body, &item_ref),
            }
        })
        .collect();

    let value_lists = value_list_def
        .captures_iter(source)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineValueList {
                oid: xml_attr(attrs, "OID"),
                item_refs: parse_item_refs(body, &item_ref),
            }
        })
        .collect();

    let where_clauses = where_clause_def
        .captures_iter(source)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineWhereClause {
                oid: xml_attr(attrs, "OID"),
                range_checks: parse_range_checks(body, &range_check, &check_value),
            }
        })
        .collect();

    let methods = method_def
        .captures_iter(source)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineMethod {
                oid: xml_attr(attrs, "OID"),
                name: xml_attr(attrs, "Name"),
                method_type: xml_attr(attrs, "Type"),
                formal_expressions: parse_formal_expressions(body, &formal_expression),
            }
        })
        .collect();

    Ok(DefineXmlMetadata {
        variables,
        codelists,
        datasets,
        value_lists,
        where_clauses,
        methods,
    })
}

fn define_variable_from_item(attrs: &str, body: &str, codelist_ref: &Regex) -> DefineVariable {
    let codelist_oid = xml_attr(attrs, "CodeListOID").or_else(|| {
        codelist_ref
            .captures(body)
            .and_then(|capture| capture.name("attrs"))
            .and_then(|attrs| xml_attr(attrs.as_str(), "CodeListOID"))
    });

    DefineVariable {
        oid: xml_attr(attrs, "OID"),
        name: xml_attr(attrs, "Name").unwrap_or_default(),
        data_type: xml_attr(attrs, "DataType"),
        length: xml_attr(attrs, "Length"),
        codelist_oid,
    }
}

fn parse_item_refs(body: &str, item_ref: &Regex) -> Vec<DefineItemRef> {
    item_ref
        .captures_iter(body)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineItemRef {
                item_oid: xml_attr(attrs, "ItemOID"),
                order_number: xml_attr(attrs, "OrderNumber"),
                mandatory: xml_attr(attrs, "Mandatory"),
                method_oid: xml_attr(attrs, "MethodOID"),
                where_clause_oid: xml_attr(attrs, "WhereClauseOID"),
                value_list_oid: xml_attr(attrs, "ValueListOID"),
            }
        })
        .collect()
}

fn parse_range_checks(
    body: &str,
    range_check: &Regex,
    check_value: &Regex,
) -> Vec<DefineRangeCheck> {
    range_check
        .captures_iter(body)
        .map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            DefineRangeCheck {
                item_oid: xml_attr(attrs, "def:ItemOID").or_else(|| xml_attr(attrs, "ItemOID")),
                comparator: xml_attr(attrs, "Comparator"),
                soft_hard: xml_attr(attrs, "SoftHard"),
                check_values: check_value
                    .captures_iter(body)
                    .filter_map(|capture| capture.name("value"))
                    .map(|value| value.as_str().trim().to_owned())
                    .filter(|value| !value.is_empty())
                    .collect(),
            }
        })
        .collect()
}

fn parse_formal_expressions(body: &str, formal_expression: &Regex) -> Vec<DefineFormalExpression> {
    formal_expression
        .captures_iter(body)
        .filter_map(|capture| {
            let attrs = capture
                .name("attrs")
                .map(|value| value.as_str())
                .unwrap_or("");
            let expression = capture.name("body")?.as_str().trim().to_owned();
            (!expression.is_empty()).then_some(DefineFormalExpression {
                context: xml_attr(attrs, "Context"),
                expression,
            })
        })
        .collect()
}

pub fn load_ct_json_file(path: impl AsRef<Path>) -> Result<ControlledTerminology> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| CdiscLibraryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: Value = serde_json::from_str(&source).map_err(|source| CdiscLibraryError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(parse_ct_json_value(&value))
}

pub fn parse_ct_json_value(value: &Value) -> ControlledTerminology {
    let mut terminology = ControlledTerminology::default();
    if let Some(object) = value.as_object() {
        if let Some(codelists) = object.get("codelists").and_then(Value::as_array) {
            insert_cdisc_ct_codelists(&mut terminology, codelists);
        }
        for (codelist, values) in object {
            insert_ct_values(&mut terminology, codelist, values);
        }
    }
    terminology
}

fn insert_cdisc_ct_codelists(terminology: &mut ControlledTerminology, codelists: &[Value]) {
    for codelist in codelists {
        let Some(object) = codelist.as_object() else {
            continue;
        };
        let Some(codelist_name) = object
            .get("submissionValue")
            .or_else(|| object.get("name"))
            .or_else(|| object.get("conceptId"))
            .or_else(|| object.get("codelist"))
            .and_then(Value::as_str)
        else {
            continue;
        };

        for key in ["terms", "enumeratedItems", "codeListItems"] {
            if let Some(terms) = object.get(key).and_then(Value::as_array) {
                insert_ct_values(terminology, codelist_name, &Value::Array(terms.clone()));
            }
        }
    }
}

fn insert_ct_values(terminology: &mut ControlledTerminology, codelist: &str, values: &Value) {
    match values {
        Value::Array(values) => {
            for value in values {
                if let Some(term) = value
                    .as_str()
                    .or_else(|| value.get("value").and_then(Value::as_str))
                    .or_else(|| value.get("CodedValue").and_then(Value::as_str))
                    .or_else(|| value.get("codedValue").and_then(Value::as_str))
                    .or_else(|| value.get("submissionValue").and_then(Value::as_str))
                    .or_else(|| value.get("code").and_then(Value::as_str))
                {
                    terminology
                        .codelists
                        .entry(codelist.to_owned())
                        .or_default()
                        .insert(term.to_owned());
                }
            }
        }
        Value::Object(object) => {
            for (nested, values) in object {
                insert_ct_values(terminology, nested, values);
            }
        }
        _ => {}
    }
}

fn xml_attr(attrs: &str, name: &str) -> Option<String> {
    let pattern = Regex::new(&format!(r#"{name}\s*=\s*["']([^"']*)["']"#)).ok()?;
    pattern
        .captures(attrs)
        .and_then(|capture| capture.get(1))
        .map(|value| value.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use serde_json::json;

    use super::*;

    #[test]
    fn parse_define_xml_extracts_variables_and_codelists() {
        let define = r#"
<ODM>
  <ItemDef OID="IT.AE.DOMAIN" Name="DOMAIN" DataType="text" Length="2"/>
  <CodeList OID="CL.DOMAIN">
    <CodeListItem CodedValue="AE"/>
    <CodeListItem CodedValue="CM"/>
  </CodeList>
</ODM>
"#;

        let metadata = parse_define_xml(define).expect("parse define");

        assert_eq!(metadata.variables.len(), 1);
        assert_eq!(metadata.variables[0].name, "DOMAIN");
        assert_eq!(metadata.variables[0].data_type.as_deref(), Some("text"));
        assert_eq!(metadata.codelists.len(), 2);
        assert_eq!(metadata.codelists[0].value, "AE");
    }

    #[test]
    fn parse_define_xml_extracts_nested_codelist_refs() {
        let define = r#"
<ODM>
  <ItemDef OID='IT.AE.DOMAIN' Name='DOMAIN' DataType='text'>
    <CodeListRef CodeListOID='CL.DOMAIN'/>
  </ItemDef>
  <CodeList OID='CL.DOMAIN'>
    <EnumeratedItem CodedValue='AE'/>
    <CodeListItem CodedValue='CM'/>
  </CodeList>
</ODM>
"#;

        let metadata = parse_define_xml(define).expect("parse define");

        assert_eq!(metadata.variables.len(), 1);
        assert_eq!(
            metadata.variables[0].codelist_oid.as_deref(),
            Some("CL.DOMAIN")
        );
        assert_eq!(metadata.codelists.len(), 2);
        assert_eq!(metadata.codelists[0].value, "AE");
        assert_eq!(metadata.codelists[1].value, "CM");
    }

    #[test]
    fn parse_define_xml_extracts_dataset_value_list_where_clause_and_method_metadata() {
        let define = r#"
<ODM xmlns:def="http://www.cdisc.org/ns/def/v2.1">
  <ItemGroupDef OID="IG.AE" Name="AE" Domain="AE" Purpose="Tabulation" Repeating="Yes">
    <ItemRef ItemOID="IT.AE.USUBJID" OrderNumber="1" Mandatory="Yes"/>
    <ItemRef ItemOID="IT.AE.AEDECOD" OrderNumber="2" Mandatory="No" MethodOID="MT.AEDECOD" WhereClauseOID="WC.AESER"/>
  </ItemGroupDef>
  <ValueListDef OID="VL.AE.AEDECOD">
    <ItemRef ItemOID="IT.AE.AEDECOD" OrderNumber="1" Mandatory="No" WhereClauseOID="WC.AESER"/>
  </ValueListDef>
  <WhereClauseDef OID="WC.AESER">
    <RangeCheck def:ItemOID="IT.AE.AESER" Comparator="EQ" SoftHard="Soft">
      <CheckValue>Y</CheckValue>
    </RangeCheck>
  </WhereClauseDef>
  <MethodDef OID="MT.AEDECOD" Name="Derive AEDECOD" Type="Computation">
    <FormalExpression Context="Python">AEDECOD = AETERM.upper()</FormalExpression>
  </MethodDef>
</ODM>
"#;

        let metadata = parse_define_xml(define).expect("parse define");

        assert_eq!(metadata.datasets.len(), 1);
        assert_eq!(metadata.datasets[0].domain.as_deref(), Some("AE"));
        assert_eq!(metadata.datasets[0].item_refs.len(), 2);
        assert_eq!(
            metadata.datasets[0].item_refs[1].method_oid.as_deref(),
            Some("MT.AEDECOD")
        );
        assert_eq!(metadata.value_lists.len(), 1);
        assert_eq!(
            metadata.value_lists[0].oid.as_deref(),
            Some("VL.AE.AEDECOD")
        );
        assert_eq!(metadata.where_clauses.len(), 1);
        assert_eq!(
            metadata.where_clauses[0].range_checks[0].check_values,
            vec!["Y".to_owned()]
        );
        assert_eq!(metadata.methods.len(), 1);
        assert_eq!(
            metadata.methods[0].formal_expressions[0].expression,
            "AEDECOD = AETERM.upper()"
        );
    }

    #[test]
    fn parse_ct_json_value_accepts_simple_and_object_terms() {
        let terminology = parse_ct_json_value(&json!({
            "DOMAIN": ["AE", { "CodedValue": "CM" }],
            "nested": {
                "YN": [{ "value": "Y" }, { "value": "N" }]
            }
        }));

        assert!(terminology.contains("DOMAIN", "AE"));
        assert!(terminology.contains("DOMAIN", "CM"));
        assert!(terminology.contains("YN", "Y"));
        assert!(!terminology.contains("YN", "U"));
    }

    #[test]
    fn parse_ct_json_value_accepts_cdisc_codelists_shape() {
        let terminology = parse_ct_json_value(&json!({
            "codelists": [
                {
                    "submissionValue": "DOMAIN",
                    "terms": [
                        { "submissionValue": "AE" },
                        { "codedValue": "CM" }
                    ]
                },
                {
                    "conceptId": "NY",
                    "enumeratedItems": [
                        { "code": "N" },
                        { "code": "Y" }
                    ]
                }
            ]
        }));

        assert!(terminology.contains("DOMAIN", "AE"));
        assert!(terminology.contains("DOMAIN", "CM"));
        assert!(terminology.contains("NY", "N"));
        assert!(terminology.contains("NY", "Y"));
    }
}
