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

    Ok(DefineXmlMetadata {
        variables,
        codelists,
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
        for (codelist, values) in object {
            insert_ct_values(&mut terminology, codelist, values);
        }
    }
    terminology
}

fn insert_ct_values(terminology: &mut ControlledTerminology, codelist: &str, values: &Value) {
    match values {
        Value::Array(values) => {
            for value in values {
                if let Some(term) = value
                    .as_str()
                    .or_else(|| value.get("value").and_then(Value::as_str))
                    .or_else(|| value.get("CodedValue").and_then(Value::as_str))
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
}
