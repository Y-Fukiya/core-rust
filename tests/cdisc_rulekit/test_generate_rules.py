import csv
import json

import yaml

from cdisc_rulekit.generate_rules import generate_rules, operator_set_from_inventory_rows
from cdisc_rulekit.models import CanonicalRule
from cdisc_rulekit.validate_generated import validate_generated_rules


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
    assert rule_yml["Check"]["all"][0]["operator"] == "is_empty"
    assert rule_yml["Check"]["all"][0]["name"] == "RFICDTC"
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
    match_check = next(check for check in checks if check["name"] == "TDSTOFF")
    assert match_check["operator"] == "is_not_contained_by"
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
    assert {"name": "DSDECOD", "operator": "equal_to", "value": "INFORMED CONSENT OBTAINED"} in checks
    assert {"name": "DSSTDTC", "operator": "is_empty"} in checks

    with (generated_dir / "negative" / "01" / "data" / "ds.csv").open(newline="", encoding="utf-8") as handle:
        row = next(csv.DictReader(handle))
    assert row["DSDECOD"] == "INFORMED CONSENT OBTAINED"
    assert row["DSSTDTC"] == ""


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
    assert {"name": "DIPARMCD", "operator": "not_exists"} in rule_yml["Check"]["all"]


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
