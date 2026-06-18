#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, CdiscLibraryError>;

#[derive(Debug, Error)]
pub enum CdiscLibraryError {
    #[error("unsupported CDISC library file extension: {0}")]
    UnsupportedExtension(String),
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
    #[error("failed to parse Define-XML: {0}")]
    Xml(#[from] roxmltree::Error),
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
    let document = Document::parse(source)?;

    let variables = document
        .descendants()
        .filter(|node| has_local_name(*node, "ItemDef"))
        .map(define_variable_from_item)
        .filter(|variable| !variable.name.is_empty())
        .collect::<Vec<_>>();

    let mut codelist_aliases = BTreeMap::new();
    let mut codelists = Vec::new();
    for codelist in document
        .descendants()
        .filter(|node| has_local_name(*node, "CodeList"))
    {
        let codelist_id = attr(codelist, "OID")
            .or_else(|| attr(codelist, "Name"))
            .unwrap_or_default();
        for alias in codelist_aliases_from_node(codelist, &codelist_id) {
            codelist_aliases
                .entry(codelist_id.clone())
                .or_insert_with(BTreeSet::new)
                .insert(alias);
        }

        codelists.extend(
            codelist
                .descendants()
                .filter(|node| {
                    has_local_name(*node, "CodeListItem") || has_local_name(*node, "EnumeratedItem")
                })
                .filter_map(|item| {
                    Some(ControlledTerm {
                        codelist: codelist_id.clone(),
                        value: attr(item, "CodedValue")
                            .or_else(|| attr(item, "SubmissionValue"))?,
                    })
                }),
        );
    }

    let datasets = document
        .descendants()
        .filter(|node| has_local_name(*node, "ItemGroupDef"))
        .map(|node| DefineDataset {
            oid: attr(node, "OID"),
            name: attr(node, "Name"),
            domain: attr(node, "Domain"),
            purpose: attr(node, "Purpose"),
            repeating: attr(node, "Repeating"),
            comment_oid: attr(node, "CommentOID"),
            leaf_id: attr(node, "leafID"),
            item_refs: parse_item_refs(node),
        })
        .collect();

    let value_lists = document
        .descendants()
        .filter(|node| has_local_name(*node, "ValueListDef"))
        .map(|node| DefineValueList {
            oid: attr(node, "OID"),
            item_refs: parse_item_refs(node),
        })
        .collect();

    let where_clauses = document
        .descendants()
        .filter(|node| has_local_name(*node, "WhereClauseDef"))
        .map(|node| DefineWhereClause {
            oid: attr(node, "OID"),
            range_checks: parse_range_checks(node),
        })
        .collect();

    let methods = document
        .descendants()
        .filter(|node| has_local_name(*node, "MethodDef"))
        .map(|node| DefineMethod {
            oid: attr(node, "OID"),
            name: attr(node, "Name"),
            method_type: attr(node, "Type"),
            formal_expressions: parse_formal_expressions(node),
        })
        .collect();

    let comments = document
        .descendants()
        .filter(|node| has_local_name(*node, "CommentDef"))
        .map(|node| DefineComment {
            oid: attr(node, "OID"),
            text: first_descendant_text(node, "TranslatedText"),
            document_refs: node
                .descendants()
                .filter(|child| has_local_name(*child, "DocumentRef"))
                .filter_map(|document_ref| attr(document_ref, "leafID"))
                .collect(),
        })
        .collect();

    let documents = document
        .descendants()
        .filter(|node| has_local_name(*node, "leaf"))
        .map(|node| DefineDocument {
            id: attr(node, "ID"),
            href: attr(node, "href"),
            title: first_descendant_text(node, "title"),
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

fn define_variable_from_item(node: Node<'_, '_>) -> DefineVariable {
    let codelist_oid = attr(node, "CodeListOID").or_else(|| {
        node.descendants()
            .find(|child| has_local_name(*child, "CodeListRef"))
            .and_then(|child| attr(child, "CodeListOID"))
    });

    DefineVariable {
        oid: attr(node, "OID"),
        name: attr(node, "Name").unwrap_or_default(),
        data_type: attr(node, "DataType"),
        length: attr(node, "Length"),
        codelist_oid,
    }
}

fn parse_item_refs(node: Node<'_, '_>) -> Vec<DefineItemRef> {
    node.descendants()
        .filter(|child| has_local_name(*child, "ItemRef"))
        .map(|child| DefineItemRef {
            item_oid: attr(child, "ItemOID"),
            order_number: attr(child, "OrderNumber"),
            mandatory: attr(child, "Mandatory"),
            method_oid: attr(child, "MethodOID"),
            where_clause_oid: attr(child, "WhereClauseOID"),
            value_list_oid: attr(child, "ValueListOID"),
        })
        .collect()
}

fn parse_range_checks(node: Node<'_, '_>) -> Vec<DefineRangeCheck> {
    node.descendants()
        .filter(|child| has_local_name(*child, "RangeCheck"))
        .map(|child| DefineRangeCheck {
            item_oid: attr(child, "ItemOID"),
            comparator: attr(child, "Comparator"),
            soft_hard: attr(child, "SoftHard"),
            check_values: child
                .descendants()
                .filter(|value| has_local_name(*value, "CheckValue"))
                .map(text_content)
                .filter(|value| !value.is_empty())
                .collect(),
        })
        .collect()
}

fn parse_formal_expressions(node: Node<'_, '_>) -> Vec<DefineFormalExpression> {
    node.descendants()
        .filter(|child| has_local_name(*child, "FormalExpression"))
        .filter_map(|child| {
            let expression = text_content(child);
            (!expression.is_empty()).then_some(DefineFormalExpression {
                context: attr(child, "Context"),
                expression,
            })
        })
        .collect()
}

fn codelist_aliases_from_node(node: Node<'_, '_>, canonical: &str) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    for key in [
        "OID",
        "Name",
        "SASFormatName",
        "SubmissionValue",
        "NCIExtCodeID",
    ] {
        if let Some(value) = attr(node, key) {
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

fn first_descendant_text(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.descendants()
        .find(|child| has_local_name(*child, name))
        .map(text_content)
        .filter(|value| !value.is_empty())
}

fn has_local_name(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn attr(node: Node<'_, '_>, name: &str) -> Option<String> {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .map(|attribute| attribute.value().to_owned())
}

fn text_content(node: Node<'_, '_>) -> String {
    node.descendants()
        .filter(|child| child.is_text())
        .filter_map(|child| child.text())
        .collect::<String>()
        .trim()
        .to_owned()
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

pub fn load_external_dictionary_file(path: impl AsRef<Path>) -> Result<ControlledTerminology> {
    let path = path.as_ref();
    let source = fs::read_to_string(path).map_err(|source| CdiscLibraryError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let fallback_name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("dictionary");

    match extension(path).as_deref() {
        Some("json") => {
            let value: Value =
                serde_json::from_str(&source).map_err(|source| CdiscLibraryError::Json {
                    path: path.to_path_buf(),
                    source,
                })?;
            Ok(parse_external_dictionary_json_value(&value, fallback_name))
        }
        Some("csv") => Ok(parse_external_dictionary_csv(&source, fallback_name)),
        Some(other) => Err(CdiscLibraryError::UnsupportedExtension(other.to_owned())),
        None => Err(CdiscLibraryError::UnsupportedExtension(String::new())),
    }
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

pub fn parse_external_dictionary_json_value(
    value: &Value,
    fallback_name: &str,
) -> ControlledTerminology {
    let mut terminology = ControlledTerminology::default();
    insert_external_dictionary_value(&mut terminology, fallback_name, value);
    terminology
}

fn insert_external_dictionary_value(
    terminology: &mut ControlledTerminology,
    fallback_name: &str,
    value: &Value,
) {
    match value {
        Value::Array(values) => {
            insert_ct_values(terminology, fallback_name, &Value::Array(values.to_vec()))
        }
        Value::Object(object) => {
            if let Some(dictionaries) = object_array_field(
                object,
                &["dictionaries", "Dictionaries", "externalDictionaries"],
            ) {
                for dictionary in dictionaries {
                    insert_external_dictionary_value(terminology, fallback_name, dictionary);
                }
                return;
            }

            if let Some(name) = string_field(
                object,
                &[
                    "dictionary",
                    "dictionaryName",
                    "dictionary_name",
                    "name",
                    "id",
                    "code",
                ],
            ) {
                for alias in string_fields(
                    object,
                    &[
                        "dictionary",
                        "dictionaryName",
                        "dictionary_name",
                        "name",
                        "id",
                        "code",
                        "version",
                    ],
                ) {
                    terminology.insert_alias(name, alias);
                }

                for key in ["terms", "values", "items", "codes", "entries"] {
                    if let Some(values) = object.get(key).or_else(|| {
                        object
                            .iter()
                            .find(|(candidate, _)| lookup_key(candidate) == lookup_key(key))
                            .map(|(_, value)| value)
                    }) {
                        insert_ct_values(terminology, name, values);
                    }
                }
                return;
            }

            for (dictionary, values) in object {
                insert_ct_values(terminology, dictionary, values);
            }
        }
        Value::String(value) => terminology.insert_term(fallback_name, value),
        _ => {}
    }
}

fn parse_external_dictionary_csv(source: &str, fallback_name: &str) -> ControlledTerminology {
    let mut terminology = ControlledTerminology::default();
    let mut reader = csv::ReaderBuilder::new()
        .trim(csv::Trim::Headers)
        .from_reader(source.as_bytes());
    let Ok(headers) = reader.headers() else {
        return terminology;
    };
    let headers = headers.iter().map(str::to_owned).collect::<Vec<_>>();
    let dictionary_index = find_header_index(
        &headers,
        &[
            "dictionary",
            "dictionary_name",
            "dictionaryName",
            "name",
            "source",
        ],
    );
    let value_index = find_header_index(
        &headers,
        &[
            "value",
            "term",
            "code",
            "codedValue",
            "submissionValue",
            "termCode",
        ],
    )
    .unwrap_or_else(|| if dictionary_index == Some(0) { 1 } else { 0 });

    for row in reader.records().flatten() {
        let dictionary = dictionary_index
            .and_then(|index| row.get(index))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(fallback_name);
        let Some(value) = row
            .get(value_index)
            .filter(|value| !value.trim().is_empty())
        else {
            continue;
        };
        terminology.insert_term(dictionary, value);
    }

    terminology
}

fn find_header_index(headers: &[String], candidates: &[&str]) -> Option<usize> {
    headers.iter().position(|header| {
        candidates
            .iter()
            .any(|candidate| lookup_key(header) == lookup_key(candidate))
    })
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
                if let Some(term) = value.as_str() {
                    terminology.insert_term(codelist, term);
                    continue;
                }
                if let Some(object) = value.as_object() {
                    for term in string_fields(
                        object,
                        &[
                            "value",
                            "CodedValue",
                            "codedValue",
                            "submissionValue",
                            "code",
                            "term",
                            "termCode",
                            "conceptId",
                            "preferredTerm",
                            "decode",
                        ],
                    ) {
                        terminology.insert_term(codelist, term);
                    }
                    for key in ["synonyms", "Synonyms", "aliases", "terms"] {
                        if let Some(values) = object_array_field(object, &[key]) {
                            for value in values {
                                if let Some(value) = value.as_str() {
                                    terminology.insert_term(codelist, value);
                                }
                            }
                        }
                    }
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

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
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
    fn parse_define_xml_decodes_cdata_and_attribute_entities() {
        let define = r#"
<ODM xmlns:def="http://www.cdisc.org/ns/def/v2.1">
  <MethodDef OID="MT.COMPARE" Name="Derive A &gt; B" Type="Computation">
    <FormalExpression Context="Python"><![CDATA[AVAL = 1 if A > B and C < D else 0]]></FormalExpression>
  </MethodDef>
  <CommentDef OID="COM.COMPARE">
    <Description>
      <TranslatedText><![CDATA[Use derivation when A > B & C < D.]]></TranslatedText>
    </Description>
  </CommentDef>
</ODM>
"#;

        let metadata = parse_define_xml(define).expect("parse define");

        assert_eq!(metadata.methods.len(), 1);
        assert_eq!(metadata.methods[0].name.as_deref(), Some("Derive A > B"));
        assert_eq!(
            metadata.methods[0].formal_expressions[0].expression,
            "AVAL = 1 if A > B and C < D else 0"
        );
        assert_eq!(
            metadata.comments[0].text.as_deref(),
            Some("Use derivation when A > B & C < D.")
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

    #[test]
    fn parse_external_dictionary_json_accepts_named_and_nested_shapes() {
        let terminology = parse_external_dictionary_json_value(
            &json!({
                "dictionaries": [
                    {
                        "dictionary": "MEDDRA",
                        "version": "26.1",
                        "terms": [
                            { "code": "HEADACHE" },
                            { "term": "NAUSEA" }
                        ]
                    },
                    {
                        "name": "UNII",
                        "values": ["ABC123"]
                    }
                ]
            }),
            "fallback",
        );

        assert!(terminology.contains("MEDDRA", "HEADACHE"));
        assert!(terminology.contains("26.1", "NAUSEA"));
        assert!(terminology.contains("UNII", "ABC123"));
    }

    #[test]
    fn parse_ct_and_dictionary_terms_include_synonyms_and_decodes() {
        let terminology = parse_ct_json_value(&json!({
            "codelists": [
                {
                    "submissionValue": "NY",
                    "terms": [
                        {
                            "submissionValue": "Y",
                            "termCode": "C49488",
                            "preferredTerm": "Yes",
                            "synonyms": ["YES"]
                        }
                    ]
                }
            ]
        }));

        assert!(terminology.contains("NY", "Y"));
        assert!(terminology.contains("NY", "C49488"));
        assert!(terminology.contains("NY", "Yes"));
        assert!(terminology.contains("NY", "YES"));

        let dictionary = parse_external_dictionary_json_value(
            &json!({
                "dictionary": "MEDDRA",
                "terms": [
                    {
                        "code": "10019211",
                        "preferredTerm": "Headache",
                        "decode": "HEADACHE",
                        "synonyms": ["Cephalalgia"]
                    }
                ]
            }),
            "fallback",
        );

        assert!(dictionary.contains("MEDDRA", "10019211"));
        assert!(dictionary.contains("MEDDRA", "Headache"));
        assert!(dictionary.contains("MEDDRA", "HEADACHE"));
        assert!(dictionary.contains("MEDDRA", "Cephalalgia"));
    }

    #[test]
    fn load_external_dictionary_csv_uses_dictionary_and_term_columns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dictionaries.csv");
        std::fs::write(
            &path,
            "dictionary,term\nMEDDRA,HEADACHE\nMEDDRA,NAUSEA\nUNII,ABC123\n",
        )
        .expect("write dictionary");

        let terminology = load_external_dictionary_file(&path).expect("load dictionary");

        assert!(terminology.contains("MEDDRA", "HEADACHE"));
        assert!(terminology.contains("MEDDRA", "NAUSEA"));
        assert!(terminology.contains("UNII", "ABC123"));
    }

    #[test]
    fn load_external_dictionary_csv_handles_quoted_commas_and_newlines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("dictionaries.csv");
        std::fs::write(
            &path,
            "dictionary,term\nMEDDRA,\"HEAD,ACHE\"\nMEDDRA,\"LINE\nBREAK\"\n",
        )
        .expect("write dictionary");

        let terminology = load_external_dictionary_file(&path).expect("load dictionary");

        assert!(terminology.contains("MEDDRA", "HEAD,ACHE"));
        assert!(terminology.contains("MEDDRA", "LINE\nBREAK"));
    }
}
