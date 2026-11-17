use std::collections::BTreeMap;

use core_data::{dataset_column_values, LoadedDataset};
use core_rule_model::ExecutableRule;
use serde_json::Value;

use crate::operation_fields::clean_operation_identifier;
use crate::scope_filter::{
    class_scope_matches, domain_scope_allows, scope_contains_all, scope_values,
};
use crate::{dataset_column_name, dataset_domain_value, push_unique_string, ValidateRequest};

pub(crate) fn model_allowed_variables(dataset: &LoadedDataset) -> Vec<String> {
    if dataset_domain_value(dataset) == "VS" {
        return [
            "STUDYID", "DOMAIN", "USUBJID", "POOLID", "SPDEVID", "VSSEQ", "VSGRPID", "VSREFID",
            "VSSPID", "VSTESTCD", "VSTEST", "VSCAT", "VSSCAT", "VSPOS", "VSORRES", "VSORRESU",
            "VSSTRESC", "VSSTRESN", "VSSTRESU", "VSSTAT", "VSREASND", "VSLOC", "VSLAT", "VSDIR",
            "VSPORTOT", "VSMETHOD", "VSBLFL", "VSDRVFL", "VSLOBXFL", "VSFAST", "VSEVAL",
            "VSEVALID", "VSACPTFL", "VSREPNUM", "VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH",
            "VSDTC", "VSDY", "VSTPT", "VSTPTNUM", "VSELTM", "VSTPTREF", "VSRFTDTC",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
    }

    dataset_variable_names_in_order(dataset)
        .into_iter()
        .filter(|name| {
            let upper = name.to_ascii_uppercase();
            !upper.starts_with('X') && !upper.ends_with("XX")
        })
        .collect()
}

pub(crate) fn dataset_domain_values(dataset: &LoadedDataset) -> Vec<String> {
    let mut values = Vec::new();
    if let Ok(column_values) = dataset_column_values(dataset, "DOMAIN") {
        for value in column_values {
            if let Some(value) = value
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique_string(&mut values, &value.to_ascii_uppercase());
            }
        }
    }
    if values.is_empty() {
        values.push(dataset_domain_value(dataset));
    }
    values
}

pub(crate) fn is_custom_domain(domain: &str) -> bool {
    !matches!(
        domain.to_ascii_uppercase().as_str(),
        "AE" | "AG"
            | "BE"
            | "BG"
            | "CE"
            | "CL"
            | "CM"
            | "CO"
            | "CV"
            | "DA"
            | "DD"
            | "DM"
            | "DS"
            | "DV"
            | "EC"
            | "EG"
            | "EX"
            | "FA"
            | "FT"
            | "HO"
            | "IE"
            | "IS"
            | "LB"
            | "MA"
            | "MB"
            | "MH"
            | "MI"
            | "MO"
            | "MS"
            | "OM"
            | "PC"
            | "PD"
            | "PE"
            | "PP"
            | "PR"
            | "QS"
            | "RE"
            | "RELREC"
            | "RP"
            | "RS"
            | "SC"
            | "SE"
            | "SR"
            | "SS"
            | "SU"
            | "SV"
            | "TA"
            | "TE"
            | "TF"
            | "TI"
            | "TR"
            | "TS"
            | "TU"
            | "TV"
            | "UR"
            | "VS"
    )
}

pub(crate) fn uses_tig_sdtm_domain_prefix_metadata(request: &ValidateRequest) -> bool {
    request
        .standard
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case("TIG"))
        && request.standard_version.as_deref() == Some("1.0")
        && request
            .substandard
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("SDTM"))
}

pub(crate) fn tig_custom_domain_use_case_matches(
    rule: &ExecutableRule,
    requested: Option<&str>,
) -> bool {
    requested
        .zip(rule.use_case.as_deref())
        .is_some_and(|(requested, allowed)| {
            allowed
                .split(',')
                .any(|use_case| use_case.trim().eq_ignore_ascii_case(requested))
        })
}

pub(crate) fn tig_sdtm_domain_prefix_customness(
    rule: &ExecutableRule,
    dataset: &LoadedDataset,
) -> Option<bool> {
    if !domain_scope_allows(rule.domains.as_ref(), dataset) {
        return None;
    }

    let dataset_name = dataset.metadata.name.to_ascii_uppercase();
    let domain = first_domain_code(dataset, "DOMAIN");
    let variables = dataset_variable_names_in_order(dataset);
    let has_variable = |name: &str| variables.iter().any(|var| var.eq_ignore_ascii_case(name));
    let supp_prefix = ["SUPP", "SQ"]
        .into_iter()
        .find(|prefix| dataset_name.starts_with(*prefix));
    let supplemental_domain = supp_prefix.and_then(|prefix| {
        first_domain_code(dataset, "RDOMAIN").map(|rdomain| format!("{prefix}{rdomain}"))
    });
    let unsplit_name = domain
        .as_deref()
        .or(supplemental_domain.as_deref())
        .unwrap_or(&dataset_name);
    let ap_suffix =
        if supp_prefix.is_none() && (has_variable("APID") || dataset_name.starts_with("AP")) {
            domain
                .as_deref()
                .and_then(|domain| domain.get(2..))
                .filter(|suffix| suffix.len() >= 2)
                .or_else(|| {
                    dataset_name
                        .strip_prefix("AP")
                        .filter(|suffix| suffix.len() >= 2)
                })
        } else {
            None
        };

    let includes = scope_values(rule.classes.as_ref(), "Include");
    if !scope_contains_all(&includes) {
        let class = tig_sdtm_standard_class(unsplit_name)
            .or_else(|| infer_sdtm_topic_class(domain.as_deref(), &has_variable))
            .or_else(|| {
                let suffix = ap_suffix?;
                tig_sdtm_standard_class(suffix)
                    .or_else(|| infer_sdtm_topic_class(Some(suffix), &has_variable))
            });
        let excludes = scope_values(rule.classes.as_ref(), "Exclude");
        if (!includes.is_empty() && !class.is_some_and(|c| class_scope_matches(&includes, c)))
            || class.is_some_and(|c| class_scope_matches(&excludes, c))
        {
            return None;
        }
    }

    let check_name = if supp_prefix.is_some() {
        "SUPPQUAL"
    } else {
        ap_suffix.unwrap_or(unsplit_name)
    };
    Some(tig_sdtm_standard_class(check_name).is_none())
}

fn first_domain_code(dataset: &LoadedDataset, column: &str) -> Option<String> {
    let name = dataset_column_name(dataset, column)?;
    let value = dataset.frame().column(&name).ok()?.get(0).ok()?;
    value
        .get_str()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_ascii_uppercase)
}

fn infer_sdtm_topic_class(
    domain: Option<&str>,
    has_variable: &impl Fn(&str) -> bool,
) -> Option<&'static str> {
    let has_topic = |suffix| {
        if has_variable("DOMAIN") {
            domain.is_some_and(|domain| has_variable(&format!("{domain}{suffix}")))
        } else {
            has_variable("RDOMAIN") && has_variable(suffix)
        }
    };
    if has_topic("TERM") {
        Some("EVENTS")
    } else if has_topic("TRT") {
        Some("INTERVENTIONS")
    } else if has_topic("QNAM") {
        Some("RELATIONSHIP")
    } else if has_topic("TESTCD") {
        Some(if has_topic("OBJ") {
            "FINDINGS ABOUT"
        } else {
            "FINDINGS"
        })
    } else {
        None
    }
}

// CORE v1.0.0 cache: standards/tig/1-0/sdtm first, then models/sdtm/2-1.
fn tig_sdtm_standard_class(domain: &str) -> Option<&'static str> {
    match domain {
        "CM" | "EC" | "EX" | "SU" => Some("INTERVENTIONS"),
        "AE" | "DS" | "DV" | "EM" | "MH" => Some("EVENTS"),
        "DA" | "DO" | "DU" | "EG" | "IE" | "LB" | "PC" | "PP" | "PT" | "QS" | "RE" | "SC"
        | "VS" => Some("FINDINGS"),
        "FA" => Some("FINDINGS ABOUT"),
        "CO" | "DM" | "IN" | "IQ" | "IT" | "PD" | "SE" | "SV" => Some("SPECIAL PURPOSE"),
        "TA" | "TE" | "TI" | "TS" | "TV" => Some("TRIAL DESIGN"),
        "DI" | "ES" | "TO" => Some("STUDY REFERENCE"),
        "RELREC" | "SUPPQUAL" => Some("RELATIONSHIP"),
        "SJ" | "SM" => Some("SPECIAL-PURPOSE"),
        "AC" | "TD" | "TM" | "TP" | "TT" | "TX" => Some("TRIAL DESIGN"),
        "OI" => Some("STUDY REFERENCE"),
        "APRELSUB" | "DR" | "POOLDEF" | "RELREF" | "RELSPEC" | "RELSUB" => Some("RELATIONSHIP"),
        _ => None,
    }
}

pub(crate) fn insert_metadata_operation_value(
    row: &mut BTreeMap<String, Value>,
    output: &str,
    value: Value,
) {
    row.insert(output.to_owned(), value.clone());
    let clean = clean_operation_identifier(output);
    if clean != output {
        row.insert(clean, value);
    }
}

pub(crate) fn model_filtered_variable_names(
    dataset: &LoadedDataset,
    key_name: &str,
    key_value: &str,
) -> Vec<String> {
    if key_name.eq_ignore_ascii_case("role") && key_value.eq_ignore_ascii_case("Timing") {
        return timing_model_variables(dataset);
    }
    Vec::new()
}

fn timing_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let mut variables = ["VISITNUM", "VISIT", "VISITDY", "TAETORD", "EPOCH"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    variables.extend([
        format!("{domain}DTC"),
        format!("{domain}STDTC"),
        format!("{domain}ENDTC"),
        format!("{domain}DY"),
        format!("{domain}STDY"),
        format!("{domain}ENDY"),
        format!("{domain}TPT"),
        format!("{domain}TPTNUM"),
        format!("{domain}ELTM"),
        format!("{domain}TPTREF"),
        format!("{domain}RFTDTC"),
        format!("{domain}ENRF"),
        format!("{domain}PDUR"),
        format!("{domain}ENRTPT"),
        format!("{domain}ENTPT"),
    ]);
    variables
}

pub(crate) fn model_column_order_from_library(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset_domain_value(dataset);
    match domain.as_str() {
        "AE" => vec!["STUDYID", "DOMAIN", "USUBJID", "AETERM"],
        "CE" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "CESEQ", "CEGRPID", "CETERM", "CESEV", "EPOCH",
            "CEDTC", "CESTDTC", "CEENDTC", "CEDY", "CESTDY", "CEENDY", "CESTRTPT", "CESTTPT",
            "CEENRTPT", "CEENTPT",
        ],
        "CM" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "CMSEQ", "CMTRT", "CMINDC", "CMDOSE", "CMDOSU",
            "CMDOSFRQ", "CMROUTE", "EPOCH", "CMDTC", "CMSTDTC", "CMENDTC", "CMDY", "CMSTDY",
            "CMENDY", "CMSTRTPT", "CMSTTPT", "CMENRTPT", "CMENTPT",
        ],
        "FA" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "FASEQ", "FALNKGRP", "FATESTCD", "FATEST", "FAOBJ",
            "FAORRES", "FASTRESC", "FASTNRLO", "FASTNRHI", "FALOC", "VISITNUM", "EPOCH", "FADTC",
            "FADY", "FAENRTPT", "FAENTPT",
        ],
        "LB" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "LBSEQ", "LBTESTCD", "LBTEST", "LBCAT", "LBORRES",
            "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC", "LBSTRESN", "LBSTRESU", "LBSTNRLO",
            "LBSTNRHI", "LBNRIND", "LBLOBXFL", "VISITNUM", "VISIT", "LBDTC", "LBDY", "LBENRTPT",
            "LBENTPT",
        ],
        "SE" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "SESEQ", "ETCD", "ELEMENT", "EPOCH", "SESTDTC",
            "SEENDTC", "SESTDY", "SEENDY",
        ],
        "SV" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "VISITNUM", "VISIT", "SVSTDTC", "SVENDTC", "SVSTDY",
            "SVENDY", "SVUPDES",
        ],
        "VS" => vec![
            "STUDYID", "DOMAIN", "USUBJID", "VSSEQ", "VSTESTCD", "VSTEST", "VSPOS", "VSORRES",
            "VSORRESU", "VSSTRESC", "VSSTRESN", "VSSTRESU", "VSSTAT", "VSLOC", "VSLOBXFL",
            "VSREPNUM", "VISITNUM", "VISIT", "EPOCH", "VSDTC", "VSDY", "MIDS",
        ],
        _ => return model_allowed_variables(dataset),
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(crate) fn dataset_variable_names_in_order(dataset: &LoadedDataset) -> Vec<String> {
    if dataset.metadata.variables.is_empty() {
        return dataset.summary().columns;
    }
    dataset
        .metadata
        .variables
        .iter()
        .map(|variable| variable.name.clone())
        .filter(|name| !name.trim().is_empty())
        .collect()
}

pub(crate) fn expected_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    match domain.as_str() {
        "AE" => vec![
            "AELLT", "AELLTCD", "AEPTCD", "AEHLT", "AEHLTCD", "AEHLGT", "AEHLGTCD", "AEBODSYS",
            "AEBDSYCD", "AESOC", "AESOCCD", "AESER", "AEACN", "AEREL", "AESTDTC", "AEENDTC",
        ],
        "EX" => vec!["EXDOSE", "EXDOSU", "EXDOSFRM", "EXSTDTC", "EXENDTC"],
        "LB" => vec![
            "LBCAT", "LBORRES", "LBORRESU", "LBORNRLO", "LBORNRHI", "LBSTRESC", "LBSTRESN",
            "LBSTRESU", "LBSTNRLO", "LBSTNRHI", "LBNRIND", "LBLOBXFL", "VISITNUM", "LBDTC",
        ],
        "SUPPAE" => vec!["IDVAR", "IDVARVAL", "QEVAL"],
        "TA" => vec!["TABRANCH", "TATRANS"],
        _ => Vec::new(),
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

pub(crate) fn required_model_variables(dataset: &LoadedDataset) -> Vec<String> {
    let domain = dataset
        .metadata
        .domain
        .as_deref()
        .unwrap_or(dataset.metadata.name.as_str())
        .to_ascii_uppercase();
    let mut variables = vec!["STUDYID", "DOMAIN"];
    if domain != "DM" && !is_trial_design_domain_without_subject(&domain) {
        variables.push("USUBJID");
    }
    match domain.as_str() {
        "AE" => variables.extend(["AESEQ", "AETERM"]),
        "CM" => variables.extend(["CMSEQ", "CMTRT"]),
        "DM" => variables.extend(["USUBJID", "SUBJID", "RFSTDTC", "RFENDTC", "SITEID", "SEX"]),
        "EX" => variables.extend(["EXSEQ", "EXTRT"]),
        "LB" => variables.extend(["LBSEQ", "LBTESTCD", "LBTEST"]),
        "VS" => variables.extend(["VSSEQ", "VSTESTCD", "VSTEST"]),
        _ => {}
    }
    variables.into_iter().map(str::to_owned).collect()
}

fn is_trial_design_domain_without_subject(domain: &str) -> bool {
    matches!(domain, "TA" | "TE" | "TI" | "TS" | "TV")
}
