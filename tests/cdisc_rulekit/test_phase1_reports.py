import csv

from cdisc_rulekit.models import CanonicalRule, RuleMapping
from cdisc_rulekit.reports import write_phase1_quality_reports


def test_phase1_quality_reports_write_macro_fuzzy_reason_and_tracking_files(tmp_path):
    native = CanonicalRule(
        source="P21",
        source_rule_id="SD0001",
        source_rule_key="2204.0|FDA|SDTM-IG|SDTM-IG|3.3|SD0001|sdtmig.xml",
        p21_rule_id="SD0001",
        agency="FDA",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Required",
        domains=["DM"],
        variables=["USUBJID"],
        conversion_status="NATIVE_CORE",
        conversion_reasons=["HAS_NATIVE_CORE_MAPPING"],
        core_rule_id="CORE-000001",
    )
    manual = CanonicalRule(
        source="P21",
        source_rule_id="CT2002",
        source_rule_key="2204.0|FDA|SDTM-IG|SDTM-IG|3.3|CT2002|sdtmig.xml",
        p21_rule_id="CT2002",
        agency="FDA",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Match",
        variables=["%Variables.Config.CodeList.Extensible:Y%"],
        raw_condition={"variable": "%Variables.Config.CodeList.Extensible:Y%"},
        conversion_status="MANUAL_REQUIRED",
        conversion_reasons=[
            "FUZZY_CORE_CANDIDATE",
            "UNRESOLVED_VARIABLE_MACRO",
            "P21_MACRO_VARIABLE_CONFIG_MACRO",
        ],
        core_rule_id="CORE-000002",
    )
    skeleton = CanonicalRule(
        source="P21",
        source_rule_id="SD0011",
        source_rule_key="2204.0|FDA|SDTM-IG|SDTM-IG|3.3|SD0011|sdtmig.xml",
        p21_rule_id="SD0011",
        agency="FDA",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["DM"],
        conversion_status="SKELETON_ONLY",
        conversion_reasons=["NO_CORE_MAPPING", "NO_TARGET_VARIABLE"],
    )
    auto = CanonicalRule(
        source="P21",
        source_rule_id="SD1019",
        source_rule_key="2204.0|FDA|SDTM-IG|SDTM-IG|3.3|SD1019|sdtmig.xml",
        p21_rule_id="SD1019",
        agency="FDA",
        standard_name="SDTM-IG",
        standard_version="3.3",
        p21_rule_type="Condition",
        domains=["SV"],
        variables=["VISITDY"],
        conversion_status="AUTO_CONVERTIBLE",
        conversion_reasons=["NO_CORE_MAPPING", "SIMPLE_SAME_RECORD_CONDITION"],
    )
    mappings = [
        RuleMapping(
            p21_rule_id="SD0001",
            p21_rule_key=native.source_rule_key,
            core_rule_id="CORE-000001",
            match_type="CG_ID",
            confidence=0.95,
        ),
        RuleMapping(
            p21_rule_id="CT2002",
            p21_rule_key=manual.source_rule_key,
            core_rule_id="CORE-000002",
            match_type="FUZZY",
            confidence=0.72,
            standard_match=True,
            domain_overlap=["DM"],
            variable_overlap=["USUBJID"],
            message_similarity=0.66,
            notes=["Fuzzy candidate only; not native coverage evidence"],
        ),
    ]

    write_phase1_quality_reports(
        tmp_path,
        [native, manual, skeleton, auto],
        mappings,
        core_rule_count=3,
        testdata_file_count=4,
    )

    expected = [
        "classification_quality.md",
        "macro_inventory.csv",
        "macro_inventory_summary.md",
        "fuzzy_mapping_review.csv",
        "reason_examples.csv",
        "version_agency_summary.csv",
        "raw_rule_id_summary.csv",
        "source_rule_tracking.csv",
        "classification_boundary_review.csv",
        "classification_boundary_review.md",
    ]
    for name in expected:
        assert (tmp_path / name).exists(), name

    quality = (tmp_path / "classification_quality.md").read_text(encoding="utf-8")
    assert "Unique P21 raw rule IDs: `4`" in quality
    assert "CORE Published rules after standard filter: `3`" in quality

    with (tmp_path / "macro_inventory.csv").open(newline="", encoding="utf-8") as handle:
        macro_rows = list(csv.DictReader(handle))
    assert macro_rows[0]["macro_family"] == "VARIABLE_CONFIG_MACRO"
    assert macro_rows[0]["convertibility_impact"] == "BLOCKS_AUTOMATION"

    with (tmp_path / "fuzzy_mapping_review.csv").open(newline="", encoding="utf-8") as handle:
        fuzzy_rows = list(csv.DictReader(handle))
    assert fuzzy_rows[0]["review_decision"] == "REVIEW_ONLY_NOT_NATIVE"

    with (tmp_path / "source_rule_tracking.csv").open(newline="", encoding="utf-8") as handle:
        tracking_rows = list(csv.DictReader(handle))
    assert {row["source_rule_key"] for row in tracking_rows} == {
        native.source_rule_key,
        manual.source_rule_key,
        skeleton.source_rule_key,
        auto.source_rule_key,
    }

    with (tmp_path / "classification_boundary_review.csv").open(newline="", encoding="utf-8") as handle:
        boundary_rows = list(csv.DictReader(handle))
    boundary_buckets = {row["boundary_bucket"] for row in boundary_rows}
    assert "AUTO_CONVERTIBLE_READY" in boundary_buckets
    assert "SKELETON_MISSING_TARGET_VARIABLE" in boundary_buckets
    assert "MANUAL_P21_MACRO_DEPENDENCY" in boundary_buckets
