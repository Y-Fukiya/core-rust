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
    pub codelist_aliases: BTreeMap<String, BTreeSet<String>>,
    pub datasets: Vec<DefineDataset>,
    pub value_lists: Vec<DefineValueList>,
    pub where_clauses: Vec<DefineWhereClause>,
    pub methods: Vec<DefineMethod>,
    pub comments: Vec<DefineComment>,
    pub documents: Vec<DefineDocument>,
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
    pub comment_oid: Option<String>,
    pub leaf_id: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineComment {
    pub oid: Option<String>,
    pub text: Option<String>,
    pub document_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefineDocument {
    pub id: Option<String>,
    pub href: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ControlledTerminology {
    pub codelists: BTreeMap<String, BTreeSet<String>>,
    pub aliases: BTreeMap<String, String>,
}

impl ControlledTerminology {
    pub fn contains(&self, codelist: &str, value: &str) -> bool {
        self.values(codelist)
            .is_some_and(|values| values.contains(value))
    }

    pub fn values(&self, codelist: &str) -> Option<&BTreeSet<String>> {
        self.codelists.get(codelist).or_else(|| {
            self.aliases
                .get(&lookup_key(codelist))
                .and_then(|key| self.codelists.get(key))
        })
    }

    pub fn insert_term(&mut self, codelist: impl AsRef<str>, value: impl Into<String>) {
        let codelist = self.canonical_or_input(codelist.as_ref());
        self.codelists
            .entry(codelist)
            .or_default()
            .insert(value.into());
    }

    pub fn insert_alias(&mut self, canonical: impl AsRef<str>, alias: impl AsRef<str>) {
        let canonical = canonical.as_ref().trim();
        let alias = alias.as_ref().trim();
        if canonical.is_empty() || alias.is_empty() {
            return;
        }
        let canonical = self.canonical_or_input(canonical);
        self.codelists.entry(canonical.clone()).or_default();
        self.aliases.insert(lookup_key(alias), canonical);
    }

    fn canonical_or_input(&self, codelist: &str) -> String {
        self.aliases
            .get(&lookup_key(codelist))
            .cloned()
            .unwrap_or_else(|| codelist.to_owned())
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
    let item_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?ItemDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?ItemDef>"#,
    )?;
    let self_closing_item_def = Regex::new(r#"<(?:[\w.-]+:)?ItemDef\b(?P<attrs>[^>]*)/>"#)?;
    let item_group_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?ItemGroupDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?ItemGroupDef>"#,
    )?;
    let value_list_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?ValueListDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?ValueListDef>"#,
    )?;
    let where_clause_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?WhereClauseDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?WhereClauseDef>"#,
    )?;
    let range_check = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?RangeCheck\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?RangeCheck>"#,
    )?;
    let check_value = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?CheckValue\b[^>]*>(?P<value>.*?)</(?:[\w.-]+:)?CheckValue>"#,
    )?;
    let method_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?MethodDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?MethodDef>"#,
    )?;
    let formal_expression = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?FormalExpression\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?FormalExpression>"#,
    )?;
    let comment_def = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?CommentDef\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?CommentDef>"#,
    )?;
    let document_ref = Regex::new(r#"<(?:[\w.-]+:)?DocumentRef\b(?P<attrs>[^>]*)/?>"#)?;
    let leaf = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?leaf\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?leaf>"#,
    )?;
    let title = Regex::new(r#"(?s)<(?:[\w.-]+:)?title\b[^>]*>(?P<body>.*?)</(?:[\w.-]+:)?title>"#)?;
    let translated_text = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?TranslatedText\b[^>]*>(?P<body>.*?)</(?:[\w.-]+:)?TranslatedText>"#,
    )?;
    let item_ref = Regex::new(r#"<(?:[\w.-]+:)?ItemRef\b(?P<attrs>[^>]*)/?>"#)?;
    let codelist_ref = Regex::new(r#"<(?:[\w.-]+:)?CodeListRef\b(?P<attrs>[^>]*)/?>"#)?;
    let codelist = Regex::new(
        r#"(?s)<(?:[\w.-]+:)?CodeList\b(?P<attrs>[^>]*)>(?P<body>.*?)</(?:[\w.-]+:)?CodeList>"#,
    )?;
    let codelist_item =
        Regex::new(r#"<(?:[\w.-]+:)?(?:CodeListItem|EnumeratedItem)\b(?P<attrs>[^>]*)/?>"#)?;

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

    let mut codelist_aliases = BTreeMap::new();
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
            for alias in codelist_aliases_from_attrs(attrs, &codelist_id) {
                codelist_aliases
                    .entry(codelist_id.clone())
                    .or_insert_with(BTreeSet::new)
                    .insert(alias);
            }
            let body = capture
                .name("body")
                .map(|value| value.as_str())
                .unwrap_or("");
            codelist_item.captures_iter(body).filter_map(move |item| {
                let item_attrs = item.name("attrs")?.as_str();
                Some(ControlledTerm {
                    codelist: codelist_id.clone(),
                    value: xml_attr(item_attrs, "CodedValue")
                        .or_else(|| xml_attr(item_attrs, "SubmissionValue"))?,
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
                comment_oid: xml_attr(attrs, "CommentOID"),
                leaf_id: xml_attr(attrs, "leafID"),
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

    let comments = comment_def
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
            DefineComment {
                oid: xml_attr(attrs, "OID"),
                text: first_translated_text(body, &translated_text),
                document_refs: document_ref
                    .captures_iter(body)
                    .filter_map(|capture| capture.name("attrs"))
                    .filter_map(|attrs| xml_attr(attrs.as_str(), "leafID"))
                    .collect(),
            }
        })
        .collect();

    let documents = leaf
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
            DefineDocument {
                id: xml_attr(attrs, "ID"),
                href: xml_attr(attrs, "href"),
                title: title
                    .captures(body)
                    .and_then(|capture| capture.name("body"))
                    .map(|value| xml_text(value.as_str())),
            }
        })
        .collect();

    Ok(DefineXmlMetadata {
        variables,
        codelists,
        codelist_aliases,
        datasets,
        value_lists,
        where_clauses,
        methods,
        comments,
        documents,
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
            let expression = xml_text(capture.name("body")?.as_str().trim());
            (!expression.is_empty()).then_some(DefineFormalExpression {
                context: xml_attr(attrs, "Context"),
                expression,
            })
        })
        .collect()
}

fn codelist_aliases_from_attrs(attrs: &str, canonical: &str) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    for key in [
        "OID",
        "Name",
        "SASFormatName",
        "def:SASFormatName",
        "SubmissionValue",
        "NCIExtCodeID",
    ] {
        if let Some(value) = xml_attr(attrs, key) {
            aliases.insert(value);
        }
    }
    if let Some(last) = canonical.rsplit(['.', '-']).next() {
        if !last.is_empty() {
            aliases.insert(last.to_owned());
        }
    }
    aliases
}

fn first_translated_text(source: &str, translated_text: &Regex) -> Option<String> {
    translated_text
        .captures(source)
        .and_then(|capture| capture.name("body"))
        .map(|value| xml_text(value.as_str()))
        .filter(|value| !value.is_empty())
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
    match value {
        Value::Array(codelists) => insert_cdisc_ct_codelists(&mut terminology, codelists),
        Value::Object(object) => {
            if let Some(codelists) =
                object_array_field(object, &["codelists", "codeLists", "Codelists"])
            {
                insert_cdisc_ct_codelists(&mut terminology, codelists);
            }
            for (codelist, values) in object {
                if lookup_key(codelist) == "codelists" {
                    continue;
                }
                insert_ct_values(&mut terminology, codelist, values);
            }
        }
        _ => {}
    };
    terminology
}

fn insert_cdisc_ct_codelists(terminology: &mut ControlledTerminology, codelists: &[Value]) {
    for codelist in codelists {
        let Some(object) = codelist.as_object() else {
            continue;
        };
        let Some(codelist_name) = string_field(
            object,
            &[
                "submissionValue",
                "codedValue",
                "name",
                "conceptId",
                "codelist",
                "codelistCode",
                "nciCode",
            ],
        ) else {
            continue;
        };
        for alias in string_fields(
            object,
            &[
                "submissionValue",
                "codedValue",
                "name",
                "conceptId",
                "codelist",
                "codelistCode",
                "nciCode",
                "preferredTerm",
            ],
        ) {
            terminology.insert_alias(codelist_name, alias);
        }

        for key in [
            "terms",
            "enumeratedItems",
            "codeListItems",
            "items",
            "concepts",
        ] {
            if let Some(terms) = object_array_field(object, &[key]) {
                insert_ct_values(terminology, codelist_name, &Value::Array(terms.clone()));
            }
        }
    }
}

fn insert_ct_values(terminology: &mut ControlledTerminology, codelist: &str, values: &Value) {
    match values {
        Value::Array(values) => {
            for value in values {
                if let Some(term) = value.as_str().or_else(|| {
                    value.as_object().and_then(|object| {
                        string_field(
                            object,
                            &[
                                "value",
                                "CodedValue",
                                "codedValue",
                                "submissionValue",
                                "code",
                                "term",
                            ],
                        )
                    })
                }) {
                    terminology.insert_term(codelist, term);
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

fn object_array_field<'a>(
    object: &'a serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<&'a Vec<Value>> {
    keys.iter()
        .find_map(|key| {
            object.get(*key).or_else(|| {
                object
                    .iter()
                    .find(|(candidate, _)| lookup_key(candidate) == lookup_key(key))
                    .map(|(_, value)| value)
            })
        })
        .and_then(Value::as_array)
}

fn string_field<'a>(object: &'a serde_json::Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| {
            object.get(*key).or_else(|| {
                object
                    .iter()
                    .find(|(candidate, _)| lookup_key(candidate) == lookup_key(key))
                    .map(|(_, value)| value)
            })
        })
        .and_then(Value::as_str)
}

fn string_fields(object: &serde_json::Map<String, Value>, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .filter_map(|key| {
            object.get(*key).or_else(|| {
                object
                    .iter()
                    .find(|(candidate, _)| lookup_key(candidate) == lookup_key(key))
                    .map(|(_, value)| value)
            })
        })
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn xml_attr(attrs: &str, name: &str) -> Option<String> {
    let escaped = regex::escape(name);
    let name_pattern = if name.contains(':') {
        escaped
    } else {
        format!(r#"(?:[\w.-]+:)?{escaped}"#)
    };
    let pattern = Regex::new(&format!(r#"{name_pattern}\s*=\s*["']([^"']*)["']"#)).ok()?;
    pattern
        .captures(attrs)
        .and_then(|capture| capture.get(1))
        .map(|value| xml_text(value.as_str()))
}

fn xml_text(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn lookup_key(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    fn parse_define_xml_accepts_namespaced_metadata_aliases_comments_and_documents() {
        let define = r#"
<odm:ODM xmlns:odm="http://www.cdisc.org/ns/odm/v1.3" xmlns:def="http://www.cdisc.org/ns/def/v2.1" xmlns:xlink="http://www.w3.org/1999/xlink">
  <def:leaf ID="LF.DEFINE" xlink:href="define.pdf">
    <def:title>Annotated Define</def:title>
  </def:leaf>
  <odm:ItemGroupDef OID="IG.AE" Name="AE" Domain="AE" Purpose="Tabulation" Repeating="Yes" def:CommentOID="COM.AE" def:leafID="LF.DEFINE">
    <odm:ItemRef ItemOID="IT.AE.DOMAIN" OrderNumber="1" Mandatory="Yes"/>
  </odm:ItemGroupDef>
  <odm:ItemDef OID="IT.AE.DOMAIN" Name="DOMAIN" DataType="text">
    <odm:CodeListRef CodeListOID="CL.DOMAIN"/>
  </odm:ItemDef>
  <odm:CodeList OID="CL.DOMAIN" Name="Domain Abbreviation" SASFormatName="DOMAIN">
    <odm:CodeListItem CodedValue="AE"/>
  </odm:CodeList>
  <odm:CommentDef OID="COM.AE">
    <odm:Description>
      <odm:TranslatedText>AE dataset comment &amp; note</odm:TranslatedText>
    </odm:Description>
    <def:DocumentRef leafID="LF.DEFINE"/>
  </odm:CommentDef>
</odm:ODM>
"#;

        let metadata = parse_define_xml(define).expect("parse define");

        assert_eq!(metadata.datasets.len(), 1);
        assert_eq!(metadata.datasets[0].comment_oid.as_deref(), Some("COM.AE"));
        assert_eq!(metadata.datasets[0].leaf_id.as_deref(), Some("LF.DEFINE"));
        assert_eq!(
            metadata.variables[0].codelist_oid.as_deref(),
            Some("CL.DOMAIN")
        );
        assert!(metadata
            .codelist_aliases
            .get("CL.DOMAIN")
            .expect("codelist aliases")
            .contains("DOMAIN"));
        assert_eq!(metadata.comments.len(), 1);
        assert_eq!(
            metadata.comments[0].text.as_deref(),
            Some("AE dataset comment & note")
        );
        assert_eq!(metadata.comments[0].document_refs, vec!["LF.DEFINE"]);
        assert_eq!(metadata.documents.len(), 1);
        assert_eq!(metadata.documents[0].href.as_deref(), Some("define.pdf"));
        assert_eq!(
            metadata.documents[0].title.as_deref(),
            Some("Annotated Define")
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

    #[test]
    fn parse_ct_json_value_resolves_codelist_aliases() {
        let terminology = parse_ct_json_value(&json!({
            "codelists": [
                {
                    "conceptId": "C66734",
                    "submissionValue": "DOMAIN",
                    "name": "Domain Abbreviation",
                    "terms": [
                        { "submissionValue": "AE" },
                        { "submissionValue": "CM" }
                    ]
                }
            ]
        }));

        assert!(terminology.contains("DOMAIN", "AE"));
        assert!(terminology.contains("C66734", "CM"));
        assert!(terminology.contains("Domain Abbreviation", "AE"));
    }
}
