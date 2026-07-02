pub(crate) fn ddf_valid_codelist_dates() -> &'static [&'static str] {
    valid_codelist_dates()
}

pub(crate) fn valid_codelist_dates() -> &'static [&'static str] {
    &[
        "2014-09-26",
        "2014-12-19",
        "2015-03-27",
        "2015-06-26",
        "2015-09-25",
        "2015-12-18",
        "2016-03-25",
        "2016-06-24",
        "2016-09-30",
        "2016-12-16",
        "2017-03-31",
        "2017-06-30",
        "2017-09-29",
        "2017-12-22",
        "2018-03-30",
        "2018-06-29",
        "2018-09-28",
        "2018-12-21",
        "2019-03-29",
        "2019-06-28",
        "2019-09-27",
        "2019-12-20",
        "2020-03-27",
        "2020-06-26",
        "2020-11-06",
        "2020-12-18",
        "2021-03-26",
        "2021-06-25",
        "2021-09-24",
        "2021-12-17",
        "2022-03-25",
        "2022-06-24",
        "2022-09-30",
        "2022-12-16",
        "2023-03-31",
        "2023-06-30",
        "2023-09-29",
        "2023-12-15",
        "2024-03-29",
        "2024-09-27",
        "2025-03-28",
        "2025-09-26",
    ]
}

#[derive(Clone, Copy)]
pub(crate) struct StaticCodelist {
    pub(crate) extensible: bool,
    pub(crate) terms: &'static [StaticTerm],
}

#[cfg(test)]
impl StaticCodelist {
    pub(crate) fn find_by_code(&self, code: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.code.eq_ignore_ascii_case(code.trim()))
    }

    pub(crate) fn find_by_pref_term(&self, pref_term: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.pref_term.eq_ignore_ascii_case(pref_term.trim()))
    }

    pub(crate) fn find_by_value(&self, value: &str) -> Option<&'static StaticTerm> {
        self.terms
            .iter()
            .find(|term| term.value.eq_ignore_ascii_case(value.trim()))
    }
}

pub(crate) fn static_codelist_term_by_code(
    codelist_code: &str,
    codelist: &StaticCodelist,
    code: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.code.eq_ignore_ascii_case(code.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

pub(crate) fn static_codelist_term_by_pref_term(
    codelist_code: &str,
    codelist: &StaticCodelist,
    pref_term: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.pref_term.eq_ignore_ascii_case(pref_term.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

pub(crate) fn static_codelist_term_by_value(
    codelist_code: &str,
    codelist: &StaticCodelist,
    value: &str,
    version: Option<&str>,
) -> Option<&'static StaticTerm> {
    codelist.terms.iter().find(|term| {
        term.value.eq_ignore_ascii_case(value.trim())
            && static_codelist_term_matches_version(codelist_code, term, version)
    })
}

#[derive(Clone, Copy)]
pub(crate) struct StaticTerm {
    pub(crate) code: &'static str,
    pub(crate) value: &'static str,
    pub(crate) pref_term: &'static str,
}

pub(crate) fn static_codelist_term_matches_version(
    codelist_code: &str,
    term: &StaticTerm,
    version: Option<&str>,
) -> bool {
    if !static_codelist_matches_version(codelist_code, version) {
        return false;
    }
    let Some(version) = version.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    match codelist_code.trim().to_ascii_uppercase().as_str() {
        "C207415" => match term.code {
            "C17649" | "C48660" | "C0031X" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C207416" => match term.code {
            "C98704" | "C207613" | "C46079" => version >= "2024-09-27",
            _ => version >= "2025-09-26",
        },
        "C207417" => match term.code {
            "C68609" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C171445" => match term.code {
            "C177933" | "C171533" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C99078" => match term.code {
            "C15184" | "C307" | "C1909" => version >= "2023-12-15",
            "C17649" => version == "2023-12-15",
            "C54696" | "C16830" | "C18020" | "C1505" => version >= "2024-03-29",
            "C15238" | "C98769" | "C15313" => version >= "2024-09-27",
            "C218507" | "C15329" | "C923" => version >= "2025-09-26",
            _ => true,
        },
        "C127259" => match term.code {
            "C15197" | "C127779" => version >= "2023-12-15",
            "C15362" | "C15208" => version >= "2024-03-29",
            "C127780" | "C15407" => version >= "2024-09-27",
            _ => true,
        },
        "C71620" => match term.code {
            "C105499" => version >= "2024-09-27",
            "C176378" => version >= "2025-09-26",
            _ => true,
        },
        "C66735" => match term.code {
            "C28233" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C99076" => match term.code {
            "C82640" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C207418" => match term.code {
            "C202579" => version >= "2024-09-27",
            "C156473" => match term.value {
                "NIMP (AxMP)" => ("2024-09-27".."2025-09-26").contains(&version),
                "NIMP" => version >= "2025-09-26",
                _ => version >= "2024-09-27",
            },
            _ => true,
        },
        "C66737" => match term.code {
            "C198366" => match term.value {
                "PHASE I/II/III STUDY" => version < "2023-12-15",
                "PHASE I/II/III TRIAL" => version >= "2023-12-15",
                _ => true,
            },
            "C54721" => match term.value {
                "PHASE 0 TRIAL" => ("2023-12-15".."2024-09-27").contains(&version),
                "EARLY PHASE I" => version >= "2024-09-27",
                _ => true,
            },
            "C199989" | "C15602" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C188725" => match term.code {
            "C85827" => version >= "2024-09-27",
            "C163559" => version >= "2025-09-26",
            _ => version >= "2023-12-15",
        },
        "C188726" => match term.code {
            "C139173" | "C170559" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C207412" => version >= "2024-09-27",
        "C127260" => match term.code {
            "C71517" => version >= "2024-03-29",
            _ => version >= "2023-12-15",
        },
        "C127261" => match term.code {
            "C15273" => version >= "2024-03-29",
            "C53312" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C201264" => match term.code {
            "C201356" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C201265" => version >= "2023-12-15",
        "C207413" => match term.code {
            "C215663" | "C215664" | "C71476" => version >= "2025-09-26",
            _ => version >= "2024-09-27",
        },
        "C207414" => version == "2024-09-27",
        "C207419" => version >= "2024-09-27",
        "C215477" | "C215478" | "C215479" | "C215481" | "C215482" | "C215483" | "C215484" => {
            version >= "2025-09-26"
        }
        "C66726" => match term.code {
            "C42968" | "C48624" => version >= "2024-03-29",
            "C42998" => version >= "2024-09-27",
            _ => version >= "2023-12-15",
        },
        "C188724" => match term.code {
            "C70793" => match term.value {
                "Clinical Study Sponsor" => version == "2024-09-27",
                "Study Sponsor" => version >= "2025-09-26",
                _ => true,
            },
            _ => true,
        },
        "C188727" => match term.code {
            "C165830" => match term.value {
                "Real World Data" => ("2024-09-27".."2025-09-26").contains(&version),
                "Real-world Data" => version >= "2025-09-26",
                _ => true,
            },
            _ => true,
        },
        _ => true,
    }
}

pub(crate) fn static_codelist_matches_version(codelist_code: &str, version: Option<&str>) -> bool {
    let Some(version) = version.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    match codelist_code.trim().to_ascii_uppercase().as_str() {
        "C127259" => version >= "2023-12-15",
        "C127260" | "C127261" => version >= "2023-12-15",
        "C207416" => version >= "2024-09-27",
        "C207418" => version >= "2024-09-27",
        "C207412" => version >= "2024-09-27",
        "C207413" => version >= "2024-09-27",
        "C207414" => version == "2024-09-27",
        "C207419" => version >= "2024-09-27",
        "C215477" | "C215478" | "C215479" | "C215481" | "C215482" | "C215483" | "C215484" => {
            version >= "2025-09-26"
        }
        "C215486" => version >= "2025-09-26",
        "C215480" => version >= "2025-09-26",
        _ => true,
    }
}

pub(crate) fn static_codelist(code: &str) -> Option<StaticCodelist> {
    match code.trim().to_ascii_uppercase().as_str() {
        "C66732" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C16576",
                    value: "F",
                    pref_term: "Female",
                },
                StaticTerm {
                    code: "C20197",
                    value: "M",
                    pref_term: "Male",
                },
                StaticTerm {
                    code: "C49636",
                    value: "BOTH",
                    pref_term: "Both",
                },
            ],
        }),
        "C66736" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15714",
                    value: "BASIC SCIENCE",
                    pref_term: "Basic Research",
                },
                StaticTerm {
                    code: "C49654",
                    value: "CURE",
                    pref_term: "Cure Study",
                },
                StaticTerm {
                    code: "C139174",
                    value: "DEVICE FEASIBILITY",
                    pref_term: "Device Feasibility Study",
                },
                StaticTerm {
                    code: "C49653",
                    value: "DIAGNOSIS",
                    pref_term: "Diagnosis Study",
                },
                StaticTerm {
                    code: "C170629",
                    value: "DISEASE MODIFYING",
                    pref_term: "Disease Modifying Treatment Study",
                },
                StaticTerm {
                    code: "C15245",
                    value: "HEALTH SERVICES RESEARCH",
                    pref_term: "Health Services Research",
                },
                StaticTerm {
                    code: "C49655",
                    value: "MITIGATION",
                    pref_term: "Adverse Effect Mitigation Study",
                },
                StaticTerm {
                    code: "C49657",
                    value: "PREVENTION",
                    pref_term: "Prevention Study",
                },
                StaticTerm {
                    code: "C71485",
                    value: "SCREENING",
                    pref_term: "Screening Study",
                },
                StaticTerm {
                    code: "C71486",
                    value: "SUPPORTIVE CARE",
                    pref_term: "Supportive Care Study",
                },
                StaticTerm {
                    code: "C49656",
                    value: "TREATMENT",
                    pref_term: "Treatment Study",
                },
            ],
        }),
        "C66735" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15228",
                    value: "DOUBLE BLIND",
                    pref_term: "Double Blind Study",
                },
                StaticTerm {
                    code: "C187674",
                    value: "OBSERVER BLIND",
                    pref_term: "Observer Blind Study",
                },
                StaticTerm {
                    code: "C156592",
                    value: "OPEN LABEL TO TREATMENT AND DOUBLE BLIND TO IMP DOSE",
                    pref_term: "Open Label for Treatment And Double Blind to Dose",
                },
                StaticTerm {
                    code: "C49659",
                    value: "OPEN LABEL",
                    pref_term: "Open Label Study",
                },
                StaticTerm {
                    code: "C28233",
                    value: "SINGLE BLIND",
                    pref_term: "Single Blind Study",
                },
            ],
        }),
        "C99076" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C82637",
                    value: "CROSS-OVER",
                    pref_term: "Crossover Study",
                },
                StaticTerm {
                    code: "C82638",
                    value: "FACTORIAL",
                    pref_term: "Factorial Study",
                },
                StaticTerm {
                    code: "C82639",
                    value: "PARALLEL",
                    pref_term: "Parallel Study",
                },
                StaticTerm {
                    code: "C142568",
                    value: "SEQUENTIAL",
                    pref_term: "Group Sequential Design",
                },
                StaticTerm {
                    code: "C82640",
                    value: "SINGLE GROUP",
                    pref_term: "Single Group Study",
                },
            ],
        }),
        "C99077" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C98388",
                    value: "INTERVENTIONAL",
                    pref_term: "Interventional Study",
                },
                StaticTerm {
                    code: "C16084",
                    value: "OBSERVATIONAL",
                    pref_term: "Observational Study",
                },
                StaticTerm {
                    code: "C98722",
                    value: "EXPANDED ACCESS",
                    pref_term: "Expanded Access Study",
                },
                StaticTerm {
                    code: "C129000",
                    value: "PATIENT REGISTRY",
                    pref_term: "Patient Registry Study",
                },
            ],
        }),
        "C66729" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C188189",
                    value: "NASODUODENAL",
                    pref_term: "Nasoduodenal Route of Administration",
                },
                StaticTerm {
                    code: "C38215",
                    value: "INFILTRATION",
                    pref_term: "Infiltration Route of Administration",
                },
                StaticTerm {
                    code: "C38217",
                    value: "INTRACORONAL, DENTAL",
                    pref_term: "Intracoronal Dental Route of Administration",
                },
                StaticTerm {
                    code: "C38257",
                    value: "INTRAPERICARDIAL",
                    pref_term: "Intrapericardial Route of Administration",
                },
                StaticTerm {
                    code: "C38288",
                    value: "ORAL",
                    pref_term: "Oral Route of Administration",
                },
                StaticTerm {
                    code: "C38305",
                    value: "TRANSDERMAL",
                    pref_term: "Transdermal Route of Administration",
                },
                StaticTerm {
                    code: "C38311",
                    value: "UNKNOWN",
                    pref_term: "Unknown Route of Administration",
                },
                StaticTerm {
                    code: "C48623",
                    value: "NOT APPLICABLE",
                    pref_term: "Route of Administration Not Applicable",
                },
            ],
        }),
        "C71113" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C17998",
                    value: "UNKNOWN",
                    pref_term: "Unknown",
                },
                StaticTerm {
                    code: "C64508",
                    value: "Q18H",
                    pref_term: "Every Eighteen Hours",
                },
                StaticTerm {
                    code: "C64525",
                    value: "QOD",
                    pref_term: "Every Other Day",
                },
                StaticTerm {
                    code: "C64528",
                    value: "3 TIMES PER WEEK",
                    pref_term: "Three Times Weekly",
                },
                StaticTerm {
                    code: "C64954",
                    value: "OCCASIONAL",
                    pref_term: "Infrequent",
                },
                StaticTerm {
                    code: "C71129",
                    value: "BIM",
                    pref_term: "Twice Per Month",
                },
                StaticTerm {
                    code: "C89791",
                    value: "Q36H",
                    pref_term: "Every Thirty-six Hours",
                },
                StaticTerm {
                    code: "C98860",
                    value: "3 TIMES PER YEAR",
                    pref_term: "Three Times Yearly",
                },
            ],
        }),
        "C188723" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25425",
                    value: "Approval",
                    pref_term: "Approved",
                },
                StaticTerm {
                    code: "C25508",
                    value: "Final",
                    pref_term: "Final",
                },
                StaticTerm {
                    code: "C63553",
                    value: "Obsolete",
                    pref_term: "Obsolete",
                },
                StaticTerm {
                    code: "C85255",
                    value: "Draft",
                    pref_term: "Draft",
                },
                StaticTerm {
                    code: "C188862",
                    value: "Pending Review",
                    pref_term: "Pending Review",
                },
            ],
        }),
        "C207418" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C202579",
                    value: "IMP",
                    pref_term: "Investigational Medicinal Product",
                },
                StaticTerm {
                    code: "C156473",
                    value: "NIMP (AxMP)",
                    pref_term: "Auxiliary Medicinal Product",
                },
                StaticTerm {
                    code: "C156473",
                    value: "NIMP",
                    pref_term: "Auxiliary Medicinal Product",
                },
            ],
        }),
        "C66737" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15600",
                    value: "PHASE I TRIAL",
                    pref_term: "Phase I Trial",
                },
                StaticTerm {
                    code: "C15601",
                    value: "PHASE II TRIAL",
                    pref_term: "Phase II Trial",
                },
                StaticTerm {
                    code: "C15602",
                    value: "PHASE III TRIAL",
                    pref_term: "Phase III Trial",
                },
                StaticTerm {
                    code: "C48660",
                    value: "NOT APPLICABLE",
                    pref_term: "Not Applicable",
                },
                StaticTerm {
                    code: "C198366",
                    value: "PHASE I/II/III STUDY",
                    pref_term: "Phase I/II/III Study",
                },
                StaticTerm {
                    code: "C198366",
                    value: "PHASE I/II/III TRIAL",
                    pref_term: "Phase I/II/III Trial",
                },
                StaticTerm {
                    code: "C199989",
                    value: "PHASE IB TRIAL",
                    pref_term: "Phase Ib Trial",
                },
                StaticTerm {
                    code: "C54721",
                    value: "PHASE 0 TRIAL",
                    pref_term: "Phase 0 Trial",
                },
                StaticTerm {
                    code: "C54721",
                    value: "EARLY PHASE I",
                    pref_term: "Early Phase 1 Trial",
                },
            ],
        }),
        "C188725" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C85826",
                    value: "Study Primary Objective",
                    pref_term: "Trial Primary Objective",
                },
                StaticTerm {
                    code: "C85827",
                    value: "Study Secondary Objective",
                    pref_term: "Trial Secondary Objective",
                },
                StaticTerm {
                    code: "C163559",
                    value: "Exploratory Objective",
                    pref_term: "Trial Exploratory Objective",
                },
            ],
        }),
        "C188726" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C94496",
                    value: "Primary Endpoint",
                    pref_term: "Primary Endpoint",
                },
                StaticTerm {
                    code: "C139173",
                    value: "Secondary Endpoint",
                    pref_term: "Secondary Endpoint",
                },
                StaticTerm {
                    code: "C170559",
                    value: "Exploratory Endpoint",
                    pref_term: "Exploratory Endpoint",
                },
            ],
        }),
        "C188728" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C25716",
                value: "Visit",
                pref_term: "Visit",
            }],
        }),
        "C207412" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25464",
                    value: "Country",
                    pref_term: "Country",
                },
                StaticTerm {
                    code: "C41129",
                    value: "Region",
                    pref_term: "Region",
                },
                StaticTerm {
                    code: "C68846",
                    value: "Global",
                    pref_term: "Global",
                },
            ],
        }),
        "C66797" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25370",
                    value: "EXCLUSION",
                    pref_term: "Exclusion Criteria",
                },
                StaticTerm {
                    code: "C25532",
                    value: "INCLUSION",
                    pref_term: "Inclusion Criteria",
                },
            ],
        }),
        "C127260" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C127781",
                    value: "NON-PROBABILITY SAMPLE",
                    pref_term: "Non-Probability Sampling Method",
                },
                StaticTerm {
                    code: "C71517",
                    value: "PROBABILITY SAMPLE",
                    pref_term: "Equal Probability Sampling Method",
                },
            ],
        }),
        "C127261" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15273",
                    value: "PROSPECTIVE",
                    pref_term: "Longitudinal Study",
                },
                StaticTerm {
                    code: "C53310",
                    value: "CROSS SECTIONAL",
                    pref_term: "Cross-Sectional Study",
                },
                StaticTerm {
                    code: "C53312",
                    value: "RETROSPECTIVE",
                    pref_term: "Retrospective Study",
                },
            ],
        }),
        "C201264" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C201356",
                    value: "After",
                    pref_term: "After Timing Type",
                },
                StaticTerm {
                    code: "C201357",
                    value: "Before",
                    pref_term: "Before Timing Type",
                },
                StaticTerm {
                    code: "C201358",
                    value: "Fixed Reference",
                    pref_term: "Fixed Reference Timing Type",
                },
            ],
        }),
        "C207413" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C132352",
                    value: "Sponsor Approval Date",
                    pref_term: "Protocol Approval by Sponsor Date",
                },
                StaticTerm {
                    code: "C207598",
                    value: "Protocol Effective Date",
                    pref_term: "Protocol Effective Date",
                },
                StaticTerm {
                    code: "C215663",
                    value: "Effective Date",
                    pref_term: "Effective Date",
                },
                StaticTerm {
                    code: "C215664",
                    value: "Issued Date",
                    pref_term: "Issued Date",
                },
                StaticTerm {
                    code: "C71476",
                    value: "Approval Date",
                    pref_term: "Approval Date",
                },
            ],
        }),
        "C207419" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207615",
                    value: "Brief Study Title",
                    pref_term: "Brief Study Title",
                },
                StaticTerm {
                    code: "C207616",
                    value: "Official Study Title",
                    pref_term: "Official Study Title",
                },
                StaticTerm {
                    code: "C207617",
                    value: "Public Study Title",
                    pref_term: "Public Study Title",
                },
                StaticTerm {
                    code: "C207618",
                    value: "Scientific Study Title",
                    pref_term: "Scientific Study Title",
                },
                StaticTerm {
                    code: "C207646",
                    value: "Study Acronym",
                    pref_term: "Study Acronym",
                },
            ],
        }),
        "C215477" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C70817",
                value: "Protocol",
                pref_term: "Study Protocol",
            }],
        }),
        "C215478" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C142424",
                    value: "Clinical Development Plan",
                    pref_term: "Clinical Development Plan",
                },
                StaticTerm {
                    code: "C215674",
                    value: "Pediatric Investigation Clinical Development Plan",
                    pref_term: "Pediatric Investigation Plan",
                },
            ],
        }),
        "C215479" => Some(StaticCodelist {
            extensible: true,
            terms: &[StaticTerm {
                code: "C45997",
                value: "pH",
                pref_term: "pH",
            }],
        }),
        "C215481" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215665",
                    value: "Study Subject Safety",
                    pref_term: "Study Subject Safety",
                },
                StaticTerm {
                    code: "C215666",
                    value: "Study Subject Rights",
                    pref_term: "Study Subject Rights",
                },
                StaticTerm {
                    code: "C215667",
                    value: "Study Data Reliability",
                    pref_term: "Study Data Reliability",
                },
                StaticTerm {
                    code: "C215668",
                    value: "Study Data Robustness",
                    pref_term: "Study Data Robustness",
                },
            ],
        }),
        "C215482" | "C215483" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215659",
                    value: "Centrally Sourced",
                    pref_term: "Centrally Sourced Indicator",
                },
                StaticTerm {
                    code: "C215660",
                    value: "Locally Sourced",
                    pref_term: "Locally Sourced Indicator",
                },
            ],
        }),
        "C215484" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C104504",
                    value: "Batch Number",
                    pref_term: "Batch Number",
                },
                StaticTerm {
                    code: "C112279",
                    value: "FDA Unique Device Identification",
                    pref_term: "FDA Unique Device Identifier",
                },
                StaticTerm {
                    code: "C70848",
                    value: "Lot Number",
                    pref_term: "Lot Number",
                },
                StaticTerm {
                    code: "C99285",
                    value: "Model Number",
                    pref_term: "Model Number",
                },
            ],
        }),
        "C66726" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C158215",
                    value: "CAPSULE, SOFTGEL, EXTENDED RELEASE",
                    pref_term: "Extended Release Capsule, Softgel Dosage Form",
                },
                StaticTerm {
                    code: "C42968",
                    value: "PATCH",
                    pref_term: "Patch Dosage Form",
                },
                StaticTerm {
                    code: "C42998",
                    value: "TABLET",
                    pref_term: "Tablet Dosage Form",
                },
                StaticTerm {
                    code: "C48624",
                    value: "NOT APPLICABLE",
                    pref_term: "Dosage Form Not Applicable",
                },
            ],
        }),
        "C201265" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C201352",
                    value: "End to End",
                    pref_term: "End to End",
                },
                StaticTerm {
                    code: "C201353",
                    value: "End to Start",
                    pref_term: "End to Start",
                },
                StaticTerm {
                    code: "C201354",
                    value: "Start to End",
                    pref_term: "Start to End",
                },
                StaticTerm {
                    code: "C201355",
                    value: "Start to Start",
                    pref_term: "Start to Start",
                },
            ],
        }),
        "C207414" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C17445",
                    value: "Caregiver",
                    pref_term: "Care Provider",
                },
                StaticTerm {
                    code: "C207599",
                    value: "Outcomes Assessor",
                    pref_term: "Outcomes Assessor",
                },
                StaticTerm {
                    code: "C25936",
                    value: "Investigator",
                    pref_term: "Investigator",
                },
                StaticTerm {
                    code: "C41189",
                    value: "Study Subject",
                    pref_term: "Study Subject",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
            ],
        }),
        "C127259" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C15197",
                    value: "CASE CONTROL",
                    pref_term: "Case-Control Study",
                },
                StaticTerm {
                    code: "C127779",
                    value: "CASE CROSSOVER",
                    pref_term: "Observational Case-Crossover Study",
                },
                StaticTerm {
                    code: "C15362",
                    value: "CASE ONLY",
                    pref_term: "Case Study",
                },
                StaticTerm {
                    code: "C15208",
                    value: "COHORT",
                    pref_term: "Cohort Study",
                },
                StaticTerm {
                    code: "C127780",
                    value: "ECOLOGIC OR COMMUNITY",
                    pref_term: "Ecologic or Community Based Study",
                },
                StaticTerm {
                    code: "C15407",
                    value: "FAMILY BASED",
                    pref_term: "Family Study",
                },
            ],
        }),
        "C66781" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C25529",
                    value: "HOURS",
                    pref_term: "Hour",
                },
                StaticTerm {
                    code: "C25301",
                    value: "DAYS",
                    pref_term: "Day",
                },
                StaticTerm {
                    code: "C29844",
                    value: "WEEKS",
                    pref_term: "Week",
                },
                StaticTerm {
                    code: "C29846",
                    value: "MONTHS",
                    pref_term: "Month",
                },
                StaticTerm {
                    code: "C29848",
                    value: "YEARS",
                    pref_term: "Year",
                },
            ],
        }),
        "C71620" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C25301",
                    value: "DAYS",
                    pref_term: "Day",
                },
                StaticTerm {
                    code: "C28253",
                    value: "mg",
                    pref_term: "Milligram",
                },
                StaticTerm {
                    code: "C29844",
                    value: "WEEKS",
                    pref_term: "Week",
                },
                StaticTerm {
                    code: "C29846",
                    value: "MONTHS",
                    pref_term: "Month",
                },
                StaticTerm {
                    code: "C29848",
                    value: "YEARS",
                    pref_term: "Year",
                },
                StaticTerm {
                    code: "C176378",
                    value: "mg/mL/day",
                    pref_term: "Gram per Liter per Day",
                },
                StaticTerm {
                    code: "C25613",
                    value: "%",
                    pref_term: "Percentage",
                },
                StaticTerm {
                    code: "C198376",
                    value: "10^4 IU/mL",
                    pref_term: "Ten Thousand International Units per Milliliter",
                },
                StaticTerm {
                    code: "C44278",
                    value: "U",
                    pref_term: "Unit",
                },
                StaticTerm {
                    code: "C105499",
                    value: "uV*s",
                    pref_term: "Microvolt Second",
                },
            ],
        }),
        "C127262" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C127785",
                    value: "CHILD CARE CENTER",
                    pref_term: "Childcare Center",
                },
                StaticTerm {
                    code: "C51282",
                    value: "CLINIC",
                    pref_term: "Clinic",
                },
                StaticTerm {
                    code: "C48953",
                    value: "FARM",
                    pref_term: "Farm",
                },
                StaticTerm {
                    code: "C102650",
                    value: "FIELD",
                    pref_term: "In the Field",
                },
                StaticTerm {
                    code: "C21541",
                    value: "HEALTH FACILITY",
                    pref_term: "Healthcare Facility",
                },
                StaticTerm {
                    code: "C18002",
                    value: "HOME",
                    pref_term: "Home",
                },
                StaticTerm {
                    code: "C16696",
                    value: "HOSPITAL",
                    pref_term: "Hospital",
                },
                StaticTerm {
                    code: "C102647",
                    value: "HOUSEHOLD ENVIRONMENT",
                    pref_term: "Household Environment",
                },
                StaticTerm {
                    code: "C41206",
                    value: "INSTITUTION",
                    pref_term: "Institution",
                },
                StaticTerm {
                    code: "C181529",
                    value: "MOTOR VEHICLE",
                    pref_term: "Motor Vehicle",
                },
                StaticTerm {
                    code: "C102679",
                    value: "NON-HOUSEHOLD ENVIRONMENT",
                    pref_term: "Non-household Environment",
                },
                StaticTerm {
                    code: "C181530",
                    value: "NOT IN CLINIC",
                    pref_term: "Not In Clinic",
                },
                StaticTerm {
                    code: "C16281",
                    value: "OUTPATIENT CLINIC",
                    pref_term: "Ambulatory Care Facility",
                },
                StaticTerm {
                    code: "C85862",
                    value: "PRISON",
                    pref_term: "Correctional Institution",
                },
                StaticTerm {
                    code: "C17118",
                    value: "SCHOOL",
                    pref_term: "School",
                },
                StaticTerm {
                    code: "C85863",
                    value: "SHELTER",
                    pref_term: "Shelter",
                },
                StaticTerm {
                    code: "C102712",
                    value: "SOCIAL SETTING",
                    pref_term: "Social Setting",
                },
                StaticTerm {
                    code: "C17556",
                    value: "WORKSITE",
                    pref_term: "Worksite",
                },
            ],
        }),
        "C171445" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C25170",
                    value: "E-MAIL",
                    pref_term: "E-mail",
                },
                StaticTerm {
                    code: "C175574",
                    value: "IN PERSON",
                    pref_term: "In Person",
                },
                StaticTerm {
                    code: "C177933",
                    value: "IVRS",
                    pref_term: "Interactive Voice Response System",
                },
                StaticTerm {
                    code: "C70805",
                    value: "LETTER",
                    pref_term: "Letter",
                },
                StaticTerm {
                    code: "C171525",
                    value: "REMOTE AUDIO VIDEO",
                    pref_term: "Audio-Videoconferencing",
                },
                StaticTerm {
                    code: "C171524",
                    value: "REMOTE AUDIO",
                    pref_term: "Audioconferencing",
                },
                StaticTerm {
                    code: "C171533",
                    value: "SHIPMENT CONFIRMED BY SIGNATURE",
                    pref_term: "Shipment Confirmed by Signature",
                },
                StaticTerm {
                    code: "C171537",
                    value: "TELEPHONE CALL",
                    pref_term: "Telephone Call",
                },
                StaticTerm {
                    code: "C157352",
                    value: "TEXT MESSAGE",
                    pref_term: "Text Message",
                },
            ],
        }),
        "C99078" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C15184",
                    value: "BEHAVIORAL THERAPY",
                    pref_term: "Behavioral Intervention",
                },
                StaticTerm {
                    code: "C307",
                    value: "BIOLOGIC",
                    pref_term: "Biological Agent",
                },
                StaticTerm {
                    code: "C54696",
                    value: "COMBINATION PRODUCT",
                    pref_term: "Combination Product",
                },
                StaticTerm {
                    code: "C16830",
                    value: "DEVICE",
                    pref_term: "Medical Device",
                },
                StaticTerm {
                    code: "C18020",
                    value: "DIAGNOSTIC TEST",
                    pref_term: "Diagnostic Procedure",
                },
                StaticTerm {
                    code: "C1505",
                    value: "DIETARY SUPPLEMENT",
                    pref_term: "Dietary Supplement",
                },
                StaticTerm {
                    code: "C1909",
                    value: "DRUG",
                    pref_term: "Pharmacologic Substance",
                },
                StaticTerm {
                    code: "C15238",
                    value: "GENETIC",
                    pref_term: "Gene Therapy",
                },
                StaticTerm {
                    code: "C218507",
                    value: "NON-SURGICAL PROCEDURE",
                    pref_term: "Non-Surgical Procedure",
                },
                StaticTerm {
                    code: "C98769",
                    value: "PROCEDURE",
                    pref_term: "Physical Medical Procedure",
                },
                StaticTerm {
                    code: "C15313",
                    value: "RADIATION",
                    pref_term: "Radiation Therapy",
                },
                StaticTerm {
                    code: "C15329",
                    value: "SURGERY",
                    pref_term: "Surgical Procedure",
                },
                StaticTerm {
                    code: "C923",
                    value: "VACCINE",
                    pref_term: "Vaccine",
                },
                StaticTerm {
                    code: "C17649",
                    value: "OTHER",
                    pref_term: "Other",
                },
            ],
        }),
        "C66739" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C158283",
                    value: "ADHESION PERFORMANCE",
                    pref_term: "Adhesion Performance Study",
                },
                StaticTerm {
                    code: "C158284",
                    value: "ALCOHOL EFFECT",
                    pref_term: "Alcohol Effect Study",
                },
                StaticTerm {
                    code: "C49664",
                    value: "BIO-AVAILABILITY",
                    pref_term: "Bioavailability Study",
                },
                StaticTerm {
                    code: "C49665",
                    value: "BIO-EQUIVALENCE",
                    pref_term: "Therapeutic Equivalency Study",
                },
                StaticTerm {
                    code: "C158288",
                    value: "BIOSIMILARITY",
                    pref_term: "Biosimilarity Study",
                },
                StaticTerm {
                    code: "C158285",
                    value: "DEVICE-DRUG INTERACTION",
                    pref_term: "Device-Drug Interaction Study",
                },
                StaticTerm {
                    code: "C49653",
                    value: "DIAGNOSIS",
                    pref_term: "Diagnosis Study",
                },
                StaticTerm {
                    code: "C158289",
                    value: "DOSE FINDING",
                    pref_term: "Dose Finding Study",
                },
                StaticTerm {
                    code: "C158290",
                    value: "DOSE PROPORTIONALITY",
                    pref_term: "Dose Proportionality Study",
                },
                StaticTerm {
                    code: "C127803",
                    value: "DOSE RESPONSE",
                    pref_term: "Dose Response Study",
                },
                StaticTerm {
                    code: "C158286",
                    value: "DRUG-DRUG INTERACTION",
                    pref_term: "Drug-Drug Interaction Study",
                },
                StaticTerm {
                    code: "C178057",
                    value: "ECG",
                    pref_term: "Electrocardiographic Study",
                },
                StaticTerm {
                    code: "C49666",
                    value: "EFFICACY",
                    pref_term: "Efficacy Study",
                },
                StaticTerm {
                    code: "C98729",
                    value: "FOOD EFFECT",
                    pref_term: "Food Effect Study",
                },
                StaticTerm {
                    code: "C120842",
                    value: "IMMUNOGENICITY",
                    pref_term: "Immunogenicity Study",
                },
                StaticTerm {
                    code: "C201484",
                    value: "MASS BALANCE",
                    pref_term: "Mass Balance Study",
                },
                StaticTerm {
                    code: "C49662",
                    value: "PHARMACODYNAMIC",
                    pref_term: "Pharmacodynamic Study",
                },
                StaticTerm {
                    code: "C39493",
                    value: "PHARMACOECONOMIC",
                    pref_term: "Pharmacoeconomic Study",
                },
                StaticTerm {
                    code: "C129001",
                    value: "PHARMACOGENETIC",
                    pref_term: "Pharmacogenetic Study",
                },
                StaticTerm {
                    code: "C49661",
                    value: "PHARMACOGENOMIC",
                    pref_term: "Pharmacogenomic Study",
                },
                StaticTerm {
                    code: "C49663",
                    value: "PHARMACOKINETIC",
                    pref_term: "Pharmacokinetic Study",
                },
                StaticTerm {
                    code: "C161477",
                    value: "POSITION EFFECT",
                    pref_term: "Position Effect Trial",
                },
                StaticTerm {
                    code: "C49657",
                    value: "PREVENTION",
                    pref_term: "Prevention Study",
                },
                StaticTerm {
                    code: "C174366",
                    value: "REACTOGENICITY",
                    pref_term: "Reactogenicity Study",
                },
                StaticTerm {
                    code: "C49667",
                    value: "SAFETY",
                    pref_term: "Safety Study",
                },
                StaticTerm {
                    code: "C161478",
                    value: "SWALLOWING FUNCTION",
                    pref_term: "Swallowing Function Trial",
                },
                StaticTerm {
                    code: "C158287",
                    value: "THOROUGH QT",
                    pref_term: "Thorough QT Study",
                },
                StaticTerm {
                    code: "C98791",
                    value: "TOLERABILITY",
                    pref_term: "Tolerability Study",
                },
                StaticTerm {
                    code: "C49656",
                    value: "TREATMENT",
                    pref_term: "Treatment Study",
                },
                StaticTerm {
                    code: "C161479",
                    value: "USABILITY TESTING",
                    pref_term: "Usability Testing Study",
                },
                StaticTerm {
                    code: "C161480",
                    value: "WATER EFFECT",
                    pref_term: "Water Effect Trial",
                },
            ],
        }),
        "C207415" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207600",
                    value: "Change In Standard Of Care",
                    pref_term: "Change In Standard Of Care",
                },
                StaticTerm {
                    code: "C207601",
                    value: "Change In Strategy",
                    pref_term: "Change In Strategy",
                },
                StaticTerm {
                    code: "C207602",
                    value: "IMP Addition",
                    pref_term: "IMP Addition",
                },
                StaticTerm {
                    code: "C207603",
                    value: "Inconsistency And/or Error In The Protocol",
                    pref_term: "Inconsistency and/or Error In The Protocol",
                },
                StaticTerm {
                    code: "C207604",
                    value: "Investigator/Site Feedback",
                    pref_term: "Investigator/Site Feedback",
                },
                StaticTerm {
                    code: "C207605",
                    value: "IRB/IEC Feedback",
                    pref_term: "IRB/IEC Feedback",
                },
                StaticTerm {
                    code: "C207606",
                    value: "Manufacturing Change",
                    pref_term: "Manufacturing Change",
                },
                StaticTerm {
                    code: "C207607",
                    value: "New Data Available (Other Than Safety Data)",
                    pref_term: "New Data Available (Other Than Safety Data)",
                },
                StaticTerm {
                    code: "C207608",
                    value: "New Regulatory Guidance",
                    pref_term: "New Regulatory Guidance",
                },
                StaticTerm {
                    code: "C207609",
                    value: "New Safety Information Available",
                    pref_term: "New Safety Information Available",
                },
                StaticTerm {
                    code: "C207610",
                    value: "Protocol Design Error",
                    pref_term: "Protocol Design Error",
                },
                StaticTerm {
                    code: "C207611",
                    value: "Recruitment Difficulty",
                    pref_term: "Recruitment Difficulty",
                },
                StaticTerm {
                    code: "C207612",
                    value: "Regulatory Agency Request To Amend",
                    pref_term: "Regulatory Agency Request To Amend",
                },
                StaticTerm {
                    code: "C17649",
                    value: "OTHER",
                    pref_term: "Other",
                },
                StaticTerm {
                    code: "C48660",
                    value: "NOT APPLICABLE",
                    pref_term: "Not Applicable",
                },
                StaticTerm {
                    code: "C0031X",
                    value: "Extension",
                    pref_term: "Extension",
                },
            ],
        }),
        "C207416" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C98704",
                    value: "Adaptive",
                    pref_term: "Adaptive Design",
                },
                StaticTerm {
                    code: "C207613",
                    value: "Extension",
                    pref_term: "Extension Study Design",
                },
                StaticTerm {
                    code: "C46079",
                    value: "Randomized",
                    pref_term: "Randomized Controlled Clinical Trial",
                },
                StaticTerm {
                    code: "C217004",
                    value: "Single-Centre",
                    pref_term: "Single-Center Study",
                },
                StaticTerm {
                    code: "C217005",
                    value: "Multicentre",
                    pref_term: "Multicenter Study",
                },
                StaticTerm {
                    code: "C217006",
                    value: "Single Country",
                    pref_term: "Single Country Study",
                },
                StaticTerm {
                    code: "C217007",
                    value: "Multiple Countries",
                    pref_term: "Multiple Country Study",
                },
                StaticTerm {
                    code: "C25689",
                    value: "Stratification",
                    pref_term: "Stratification",
                },
                StaticTerm {
                    code: "C147145",
                    value: "Stratified Randomisation",
                    pref_term: "Stratified Randomization",
                },
            ],
        }),
        "C207417" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "C207614",
                    value: "Additional Required Treatment",
                    pref_term: "Additional Required Medicinal Product",
                },
                StaticTerm {
                    code: "C165822",
                    value: "Background Treatment",
                    pref_term: "Background Treatment",
                },
                StaticTerm {
                    code: "C158128",
                    value: "Challenge Agent",
                    pref_term: "Challenge Agent",
                },
                StaticTerm {
                    code: "C18020",
                    value: "Diagnostic",
                    pref_term: "Diagnostic Procedure",
                },
                StaticTerm {
                    code: "C41161",
                    value: "Experimental Intervention",
                    pref_term: "Protocol Agent",
                },
                StaticTerm {
                    code: "C753",
                    value: "Placebo",
                    pref_term: "Placebo",
                },
                StaticTerm {
                    code: "C165835",
                    value: "Rescue Medicine",
                    pref_term: "Rescue Medications",
                },
                StaticTerm {
                    code: "C68609",
                    value: "Active Comparator",
                    pref_term: "Active Comparator",
                },
            ],
        }),
        "C215486" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C215657",
                    value: "Clinical Education",
                    pref_term: "Clinical Education Study",
                },
                StaticTerm {
                    code: "C215654",
                    value: "Disease Determinants",
                    pref_term: "Disease Determinants Study",
                },
                StaticTerm {
                    code: "C215658",
                    value: "Disease Etiology",
                    pref_term: "Disease Etiology Study",
                },
                StaticTerm {
                    code: "C215653",
                    value: "Disease Incidence",
                    pref_term: "Disease Incidence Study",
                },
                StaticTerm {
                    code: "C215675",
                    value: "Disease Prevalence",
                    pref_term: "Disease Prevalence Study",
                },
                StaticTerm {
                    code: "C215655",
                    value: "Disease Prognosis",
                    pref_term: "Disease Prognosis Study",
                },
                StaticTerm {
                    code: "C215656",
                    value: "Drug Utilization",
                    pref_term: "Drug Utilization Study",
                },
                StaticTerm {
                    code: "C49667",
                    value: "Safety",
                    pref_term: "Safety Study",
                },
            ],
        }),
        "C188724" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C18240",
                    value: "Academic Institution",
                    pref_term: "Academia",
                },
                StaticTerm {
                    code: "C93453",
                    value: "Clinical Study Registry",
                    pref_term: "Study Registry",
                },
                StaticTerm {
                    code: "C54148",
                    value: "Contract Research Organization",
                    pref_term: "Contract Research Organization",
                },
                StaticTerm {
                    code: "C199144",
                    value: "Government Institute",
                    pref_term: "Governmental Agency or Group",
                },
                StaticTerm {
                    code: "C21541",
                    value: "Healthcare Facility",
                    pref_term: "Healthcare Facility",
                },
                StaticTerm {
                    code: "C37984",
                    value: "Laboratory",
                    pref_term: "Laboratory",
                },
                StaticTerm {
                    code: "C215661",
                    value: "Medical Device Company",
                    pref_term: "Medical Device Company",
                },
                StaticTerm {
                    code: "C54149",
                    value: "Drug Company",
                    pref_term: "Pharmaceutical Company",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Clinical Study Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Study Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C188863",
                    value: "Regulatory Agency",
                    pref_term: "Regulatory Agency",
                },
                StaticTerm {
                    code: "C93448",
                    value: "Research Organization",
                    pref_term: "Research Organization",
                },
            ],
        }),
        "C188727" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C165830",
                    value: "Real World Data",
                    pref_term: "Real World Data",
                },
                StaticTerm {
                    code: "C165830",
                    value: "Real-world Data",
                    pref_term: "Real-world Data",
                },
                StaticTerm {
                    code: "C176263",
                    value: "Synthetic Data",
                    pref_term: "Synthetic Data",
                },
                StaticTerm {
                    code: "C188864",
                    value: "Historical Data",
                    pref_term: "Historical Data",
                },
                StaticTerm {
                    code: "C188865",
                    value: "Virtual Data",
                    pref_term: "Virtual Data",
                },
                StaticTerm {
                    code: "C188866",
                    value: "Data Generated Within Study",
                    pref_term: "Data Generated Within Study",
                },
            ],
        }),
        "SPEC" => Some(StaticCodelist {
            extensible: false,
            terms: &[
                StaticTerm {
                    code: "",
                    value: "ABDOMINAL WALL",
                    pref_term: "Abdominal Wall",
                },
                StaticTerm {
                    code: "",
                    value: "ADIPOSE TISSUE, BROWN",
                    pref_term: "Brown Adipose Tissue",
                },
                StaticTerm {
                    code: "",
                    value: "AIR SAC",
                    pref_term: "Air Sac",
                },
            ],
        }),
        "C215480" => Some(StaticCodelist {
            extensible: true,
            terms: &[
                StaticTerm {
                    code: "C78726",
                    value: "Adjudication Committee",
                    pref_term: "Adjudication Committee",
                },
                StaticTerm {
                    code: "C17445",
                    value: "Care Provider",
                    pref_term: "Caregiver",
                },
                StaticTerm {
                    code: "C215672",
                    value: "Clinical Trial Physician",
                    pref_term: "Clinical Trial Physician",
                },
                StaticTerm {
                    code: "C215669",
                    value: "Co-Sponsor",
                    pref_term: "Study Co-Sponsor",
                },
                StaticTerm {
                    code: "C215662",
                    value: "Contract Research",
                    pref_term: "Contract Research",
                },
                StaticTerm {
                    code: "C142489",
                    value: "Data Safety Monitoring Board",
                    pref_term: "Data Monitoring Committee",
                },
                StaticTerm {
                    code: "C215671",
                    value: "Dose Escalation Committee",
                    pref_term: "Dose Escalation Committee",
                },
                StaticTerm {
                    code: "C142578",
                    value: "Independent Data Monitoring Committee",
                    pref_term: "Independent Data Monitoring Committee",
                },
                StaticTerm {
                    code: "C25936",
                    value: "Investigator",
                    pref_term: "Investigator",
                },
                StaticTerm {
                    code: "C37984",
                    value: "Laboratory",
                    pref_term: "Laboratory",
                },
                StaticTerm {
                    code: "C215670",
                    value: "Local Sponsor",
                    pref_term: "Local Legal Sponsor",
                },
                StaticTerm {
                    code: "C25392",
                    value: "Manufacturer",
                    pref_term: "Manufacturer",
                },
                StaticTerm {
                    code: "C51876",
                    value: "Medical Expert",
                    pref_term: "Sponsor Medical Expert",
                },
                StaticTerm {
                    code: "C207599",
                    value: "Outcomes Assessor",
                    pref_term: "Outcomes Assessor",
                },
                StaticTerm {
                    code: "C215673",
                    value: "Pharmacovigilance",
                    pref_term: "Pharmacovigilance Group",
                },
                StaticTerm {
                    code: "C19924",
                    value: "Principal investigator",
                    pref_term: "Principal Investigator",
                },
                StaticTerm {
                    code: "C51851",
                    value: "Project Manager",
                    pref_term: "Project Coordinator",
                },
                StaticTerm {
                    code: "C188863",
                    value: "Regulatory Agency",
                    pref_term: "Regulatory Agency",
                },
                StaticTerm {
                    code: "C70793",
                    value: "Sponsor",
                    pref_term: "Clinical Study Sponsor",
                },
                StaticTerm {
                    code: "C51877",
                    value: "Statistician",
                    pref_term: "Statistician",
                },
                StaticTerm {
                    code: "C80403",
                    value: "Study Site",
                    pref_term: "Study Site",
                },
                StaticTerm {
                    code: "C41189",
                    value: "Study Subject",
                    pref_term: "Study Subject",
                },
            ],
        }),
        _ => None,
    }
}
