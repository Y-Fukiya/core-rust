import csv
import json

import pytest
import yaml

from cdisc_rulekit.generate_rules import generate_rules, operator_set_from_inventory_rows
from cdisc_rulekit.models import CanonicalRule
from cdisc_rulekit.validate_generated import validate_generated_rules

_OPERATOR_ALIASES = {
    "does_not_match_regex": "not_matches_regex",
    "is_empty": "empty",
    "is_not_empty": "non_empty",
}


def _check(operator, **payload):
    return {**payload, "operator": _OPERATOR_ALIASES.get(operator, operator)}


def _payloads(checks, operator):
    core_operator = _OPERATOR_ALIASES.get(operator, operator)
    return [
        {key: value for key, value in check.items() if key != "operator"}
        for check in checks
        if isinstance(check, dict) and check.get("operator") == core_operator
    ]


def test_generate_rules_writes_minimal_required_rule_and_test_data(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1210",
        source_rule_key="2204.0|FDA|SDTM-IG|SDTM-IG|3.3|SD1210|sdtmig.xml",
        p21_rule_id="SD1210",
        standard_name="SDTM-IG",
        standard_version="3.3",
        agency="FDA",
        p21_rule_type="Required",
        message="Missing value for RFICDTC",
        description="RFICDTC must be populated.",
        domains=["DM"],
        variables=["RFICDTC"],
        target="RFICDTC",
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "NO_CORE_MAPPING"],
        conversion_confidence=0.7,
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    assert generated_dir.name.startswith("P21PORT-SDTMIG-SD1210-")

    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    assert rule_yml["Core"]["Id"] == generated_dir.name
    assert rule_yml["Core"]["Status"] == "Draft"
    assert rule_yml["Check"]["all"][0] == _check("is_empty", name="RFICDTC")
    assert rule_yml["Scope"]["Domains"]["Include"] == ["DM"]

    manifest = json.loads((generated_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["source_rule_key"] == rule.source_rule_key
    assert manifest["generated_rule_id"] == generated_dir.name
    assert manifest["warnings"] == []

    positive_data = generated_dir / "positive" / "01" / "data"
    negative_data = generated_dir / "negative" / "01" / "data"
    for data_dir in (positive_data, negative_data):
        assert (data_dir / ".env").exists()
        assert (data_dir / "_datasets.csv").exists()
        assert (data_dir / "_variables.csv").exists()
        assert (data_dir / "dm.csv").exists()

    with (negative_data / "dm.csv").open(newline="", encoding="utf-8") as handle:
        row = next(csv.DictReader(handle))
    assert row["RFICDTC"] == ""
    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative["variables"] == "RFICDTC|DOMAIN"

    validation = validate_generated_rules(tmp_path / "generated_rules")
    assert validation.ok


def test_validate_generated_rules_rejects_empty_generated_directory(tmp_path):
    generated_root = tmp_path / "generated_rules"
    generated_root.mkdir()

    validation = validate_generated_rules(generated_root)

    assert not validation.ok
    assert validation.checked_rule_count == 0
    assert "no generated rule directories found" in validation.issues[0]


def test_generate_rules_rejects_negative_limit(tmp_path):
    with pytest.raises(ValueError, match="limit must be zero or greater"):
        generate_rules([], tmp_path / "generated_rules", set(), limit=-1)


def test_generate_rules_skips_fuzzy_candidates_and_unknown_operators(tmp_path):
    fuzzy_rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0087",
        source_rule_key="fuzzy-key",
        p21_rule_id="SD0087",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Required",
        domains=["DM"],
        variables=["RFSTDTC"],
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["FUZZY_CORE_CANDIDATE", "NO_CORE_MAPPING"],
    )
    regex_rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1217",
        source_rule_key="regex-key",
        p21_rule_id="SD1217",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Regex",
        domains=["TS"],
        variables=["TSVAL"],
        raw_condition={"test": r"^\d+$"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_REGEX"],
    )

    summary = generate_rules(
        [fuzzy_rule, regex_rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
    )

    assert summary.generated_count == 0
    assert summary.skipped_count == 2
    reasons = {row["skip_reason"] for row in summary.rows}
    assert "FUZZY_CANDIDATE_REQUIRES_REVIEW" in reasons
    assert "OPERATOR_NOT_ALLOWED:does_not_match_regex" in reasons


def test_generate_rules_can_include_fuzzy_candidates_with_manifest_warning(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0087",
        source_rule_key="fuzzy-key",
        p21_rule_id="SD0087",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Required",
        domains=["DM"],
        variables=["RFSTDTC"],
        target="RFSTDTC",
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["FUZZY_CORE_CANDIDATE", "NO_CORE_MAPPING"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
        include_fuzzy_candidates=True,
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    manifest = json.loads((generated_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["warnings"] == ["FUZZY_CORE_CANDIDATE_REQUIRES_REVIEW"]


def test_generate_rules_writes_match_as_negative_membership_check(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDMATCH1",
        source_rule_key="match-key",
        p21_rule_id="SDMATCH1",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Match",
        domains=["TD"],
        variables=["TDSTOFF"],
        target="TDSTOFF",
        raw_condition={"terms": "Y;N"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_MATCH_TERMS"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_not_contained_by"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    match_check = next(
        payload
        for payload in _payloads(checks, "is_not_contained_by")
        if payload["name"] == "TDSTOFF"
    )
    assert set(match_check["value"]) == {"Y", "N"}

    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        expected = list(csv.DictReader(handle))
    negative = next(row for row in expected if row["case_type"] == "negative")
    assert negative["variables"] == "TDSTOFF|DOMAIN"


def test_operator_inventory_aliases_allow_canonical_empty_operator():
    operators = operator_set_from_inventory_rows([{"operator": "non_empty", "raw_keys": []}])

    assert "non_empty" in operators
    assert "is_not_empty" in operators
    assert "is_empty" in operator_set_from_inventory_rows([{"operator": "empty", "raw_keys": []}])
    assert "does_not_match_regex" in operator_set_from_inventory_rows([{"operator": "not_matches_regex", "raw_keys": []}])


def test_generate_rules_marks_numeric_variables_in_variables_csv(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDNUM1",
        source_rule_key="numeric-key",
        p21_rule_id="SDNUM1",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Required",
        domains=["SV"],
        variables=["VISITNUM"],
        target="VISITNUM",
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    with (generated_dir / "positive" / "01" / "data" / "_variables.csv").open(newline="", encoding="utf-8") as handle:
        variables = list(csv.DictReader(handle))
    visitnum = next(row for row in variables if row["variable"] == "VISITNUM")
    assert visitnum["type"] == "Num"


def test_generate_rules_supports_simple_same_record_condition(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDCOND1",
        source_rule_key="condition-key",
        p21_rule_id="SDCOND1",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["DS"],
        variables=["DSSTDTC", "DSDECOD"],
        target="DSSTDTC",
        raw_condition={"when": "DSDECOD = 'INFORMED CONSENT OBTAINED'", "test": "DSSTDTC != ''"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_SAME_RECORD_CONDITION"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    assert _check("equal_to", name="DSDECOD", value="INFORMED CONSENT OBTAINED") in checks
    assert _check("is_empty", name="DSSTDTC") in checks

    with (generated_dir / "negative" / "01" / "data" / "ds.csv").open(newline="", encoding="utf-8") as handle:
        row = next(csv.DictReader(handle))
    assert row["DSDECOD"] == "INFORMED CONSENT OBTAINED"
    assert row["DSSTDTC"] == ""


def test_generate_rules_infers_condition_target_and_inverts_empty_check(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1105",
        source_rule_key="condition-empty-key",
        p21_rule_id="SD1105",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["DS"],
        raw_condition={"when": "DSCAT == 'PROTOCOL MILESTONE'", "test": "EPOCH == ''"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_not_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    assert _check("equal_to", name="DSCAT", value="PROTOCOL MILESTONE") in checks
    assert _check("is_not_empty", name="EPOCH") in checks

    with (generated_dir / "positive" / "01" / "data" / "ds.csv").open(newline="", encoding="utf-8") as handle:
        positive = next(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "ds.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))
    assert positive["DSCAT"] == "PROTOCOL MILESTONE"
    assert positive["EPOCH"] == ""
    assert negative["DSCAT"] == "PROTOCOL MILESTONE"
    assert negative["EPOCH"] == "Y"


def test_generate_rules_supports_or_condition_guard(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1332",
        source_rule_key="condition-or-key",
        p21_rule_id="SD1332",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["AE"],
        raw_condition={"when": "AEOUT == 'NOT RECOVERED/NOT RESOLVED' @or AEOUT == 'UNKNOWN'", "test": "AEENDTC == ''"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "any", "equal_to", "is_not_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    assert {"any": [
        _check("equal_to", name="AEOUT", value="NOT RECOVERED/NOT RESOLVED"),
        _check("equal_to", name="AEOUT", value="UNKNOWN"),
    ]} in checks

    with (generated_dir / "negative" / "01" / "data" / "ae.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))
    assert negative["AEOUT"] == "NOT RECOVERED/NOT RESOLVED"
    assert negative["AEENDTC"] == "Y"


def test_generate_rules_supports_and_condition_guard(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD2240",
        source_rule_key="condition-and-key",
        p21_rule_id="SD2240",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["TS"],
        raw_condition={"when": "TSPARMCD == 'INDIC' @and TSVALNF == ''", "test": "TSVCDREF == 'SNOMED'"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty", "not_equal_to"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    assert _check("equal_to", name="TSPARMCD", value="INDIC") in checks
    assert _check("is_empty", name="TSVALNF") in checks
    assert _check("not_equal_to", name="TSVCDREF", value="SNOMED") in checks

    with (generated_dir / "negative" / "01" / "data" / "ts.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))
    assert negative["TSPARMCD"] == "INDIC"
    assert negative["TSVALNF"] == ""
    assert negative["TSVCDREF"] == "__INVALID__"


def test_generate_rules_writes_find_as_missing_presence_check(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDFIND1",
        source_rule_key="find-key",
        p21_rule_id="SDFIND1",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Find",
        domains=["DI"],
        variables=["DIPARMCD"],
        target="DIPARMCD",
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "DATASET_PRESENCE_CHECK"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "not_exists"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    assert _check("not_exists", name="DIPARMCD") in rule_yml["Check"]["all"]

    with (generated_dir / "positive" / "01" / "data" / "di.csv").open(newline="", encoding="utf-8") as handle:
        positive = next(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "di.csv").open(newline="", encoding="utf-8") as handle:
        negative_reader = csv.DictReader(handle)
        negative = next(negative_reader)

    assert "DIPARMCD" in positive
    assert "DIPARMCD" not in negative
    with (generated_dir / "negative" / "01" / "data" / "_variables.csv").open(newline="", encoding="utf-8") as handle:
        variable_names = {row["variable"] for row in csv.DictReader(handle)}
    assert "DIPARMCD" not in variable_names

    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative_expected = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative_expected["variables"] == "DOMAIN"


def test_generate_rules_wraps_regex_patterns_for_official_core_full_match(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1221",
        source_rule_key="condition-regex-key",
        p21_rule_id="SD1221",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["TS"],
        raw_condition={"when": "TSPARMCD == 'PLANSUB'", "test": "TSVAL @re '([0-9]*)'"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "does_not_match_regex"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    regex_check = next(payload for payload in _payloads(checks, "does_not_match_regex") if payload["name"] == "TSVAL")
    assert regex_check["value"] == r"^(?:([0-9]*))$"


def test_generate_rules_skips_regex_with_unsupported_rust_syntax(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDREGEX-LOOKBEHIND",
        source_rule_key="regex-lookbehind-key",
        p21_rule_id="SDREGEX-LOOKBEHIND",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Regex",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
        raw_condition={"test": r"(?<=SUBJ)\d+"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_REGEX"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "does_not_match_regex"},
    )

    assert summary.generated_count == 0
    assert summary.rows[0]["skip_reason"] == "UNSUPPORTED_RUST_REGEX_SYNTAX"


def test_generate_rules_preserves_numeric_literals_for_official_core_comparison(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1249",
        source_rule_key="condition-numeric-key",
        p21_rule_id="SD1249",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["EX"],
        raw_condition={"when": "EXTRT @eqic 'PLACEBO'", "test": "EXDOSE == '0'"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "equal_to_case_insensitive", "not_equal_to"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    dose_check = next(payload for payload in _payloads(checks, "not_equal_to") if payload["name"] == "EXDOSE")
    assert dose_check["value"] == 0


def test_generate_rules_uses_valid_duration_for_iso_duration_regex(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SDREGEX1",
        source_rule_key="regex-duration-key",
        p21_rule_id="SDREGEX1",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Regex",
        domains=["TD"],
        variables=["TDSTOFF"],
        target="TDSTOFF",
        raw_condition={"test": r"^(R\d*/)?P(?:\d+(?:\.\d+)?Y)?(?:\d+(?:\.\d+)?M)?(?:\d+(?:\.\d+)?W)?(?:\d+(?:\.\d+)?D)?$"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_REGEX"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "does_not_match_regex"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    with (generated_dir / "positive" / "01" / "data" / "td.csv").open(newline="", encoding="utf-8") as handle:
        positive = next(csv.DictReader(handle))
    assert positive["TDSTOFF"] == "P1D"


def test_generate_rules_supports_simple_or_expected_condition(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0089",
        source_rule_key="condition-or-key",
        p21_rule_id="SD0089",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["TE"],
        raw_condition={"test": "TEENRL != '' @or TEDUR != ''"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET", "SIMPLE_LOGICAL_CONDITION"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_empty"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    assert _check("is_empty", name="TEENRL") in checks
    assert _check("is_empty", name="TEDUR") in checks

    with (generated_dir / "positive" / "01" / "data" / "te.csv").open(newline="", encoding="utf-8") as handle:
        positive = next(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "te.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))

    assert positive["TEENRL"] == "Y"
    assert negative["TEENRL"] == ""
    assert negative["TEDUR"] == ""

    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative_expected = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative_expected["variables"] == "TEENRL|TEDUR|DOMAIN"


def test_generate_rules_preserves_overlapping_when_guard_for_negative_condition(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0053",
        source_rule_key="condition-overlap-key",
        p21_rule_id="SD0053",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["DM"],
        raw_condition={
            "when": "ARMCD @eqic 'NOTASSGN' @or ARM @eqic 'Not Assigned'",
            "test": "ARMCD @eqic 'NOTASSGN' @and ARM @eqic 'Not Assigned'",
        },
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET", "SIMPLE_LOGICAL_CONDITION"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "any", "equal_to", "equal_to_case_insensitive", "not_equal_to_case_insensitive"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    with (generated_dir / "negative" / "01" / "data" / "dm.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))

    assert negative["ARMCD"] == "NOTASSGN"
    assert negative["ARM"] == "__INVALID__"
    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative_expected = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative_expected["variables"] == "ARMCD|ARM|DOMAIN"


def test_generate_rules_supports_unique_group_by_checks(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0083",
        source_rule_key="unique-key",
        p21_rule_id="SD0083",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Unique",
        domains=["DM"],
        variables=["USUBJID"],
        target="USUBJID",
        raw_condition={"group_by": "STUDYID", "when": "USUBJID != ''"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_UNIQUE_SET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_not_empty", "is_not_unique_set"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    unique_check = next(payload for payload in _payloads(checks, "is_not_unique_set") if payload["name"] == "USUBJID")
    assert unique_check["value"] == "STUDYID"

    with (generated_dir / "positive" / "01" / "data" / "dm.csv").open(newline="", encoding="utf-8") as handle:
        positive_rows = list(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "dm.csv").open(newline="", encoding="utf-8") as handle:
        negative_rows = list(csv.DictReader(handle))

    assert [row["USUBJID"] for row in positive_rows] == ["P21PORT-001", "P21PORT-002"]
    assert [row["USUBJID"] for row in negative_rows] == ["P21PORT-001", "P21PORT-001"]
    assert {row["STUDYID"] for row in negative_rows} == {"CDISC-P21PORT"}

    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative_expected = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative_expected["expected_issue_count"] == "2"
    assert negative_expected["variables"] == "USUBJID|DOMAIN"

    validation = validate_generated_rules(tmp_path / "generated_rules")
    assert validation.ok


def test_generate_rules_uses_domain_for_unique_without_group_by(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD1214",
        source_rule_key="unique-domain-key",
        p21_rule_id="SD1214",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Unique",
        domains=["TS"],
        variables=["TSPARMCD"],
        target="TSPARMCD",
        raw_condition={"when": "TSPARMCD == 'ADDON'"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_UNIQUE_SET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_not_unique_set"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    unique_check = next(
        payload
        for payload in _payloads(rule_yml["Check"]["all"], "is_not_unique_set")
        if payload["name"] == "TSPARMCD"
    )
    assert unique_check["value"] == "DOMAIN"

    with (generated_dir / "positive" / "01" / "data" / "ts.csv").open(newline="", encoding="utf-8") as handle:
        positive_rows = list(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "ts.csv").open(newline="", encoding="utf-8") as handle:
        negative_rows = list(csv.DictReader(handle))

    assert [row["DOMAIN"] for row in positive_rows] == ["TS", "TS"]
    assert [row["TSPARMCD"] for row in positive_rows] == ["ADDON", "P21PORT-002"]
    assert [row["DOMAIN"] for row in negative_rows] == ["TS", "TS"]
    assert [row["TSPARMCD"] for row in negative_rows] == ["ADDON", "ADDON"]


def test_generate_rules_supports_same_record_column_equality(tmp_path):
    rule = CanonicalRule(
        source="P21",
        source_rule_id="SD0085",
        source_rule_key="condition-column-key",
        p21_rule_id="SD0085",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["IE"],
        raw_condition={"when": "IEORRES != '' @and IESTRESC != ''", "test": "IEORRES == IESTRESC"},
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "INFERRED_CONDITION_TARGET"],
    )

    summary = generate_rules(
        [rule],
        out_dir=tmp_path,
        allowed_operators={"all", "equal_to", "is_not_empty", "not_equal_to"},
    )

    assert summary.generated_count == 1
    generated_dir = next((tmp_path / "generated_rules").iterdir())
    rule_yml = yaml.safe_load((generated_dir / "rule.yml").read_text(encoding="utf-8"))
    checks = rule_yml["Check"]["all"]
    comparison = next(payload for payload in _payloads(checks, "not_equal_to") if payload["name"] == "IEORRES")
    assert comparison["value"] == "IESTRESC"

    with (generated_dir / "positive" / "01" / "data" / "ie.csv").open(newline="", encoding="utf-8") as handle:
        positive = next(csv.DictReader(handle))
    with (generated_dir / "negative" / "01" / "data" / "ie.csv").open(newline="", encoding="utf-8") as handle:
        negative = next(csv.DictReader(handle))

    assert positive["IEORRES"] == positive["IESTRESC"]
    assert negative["IEORRES"] != negative["IESTRESC"]

    with (generated_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        negative_expected = next(row for row in csv.DictReader(handle) if row["case_type"] == "negative")
    assert negative_expected["variables"] == "IEORRES|IESTRESC|DOMAIN"
