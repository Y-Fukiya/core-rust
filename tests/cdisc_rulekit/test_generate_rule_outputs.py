import csv
import json

import yaml

from cdisc_rulekit.classify import classify_rules
from cdisc_rulekit.generate_testdata import generate_rule_folder
from cdisc_rulekit.load_p21 import load_p21_rules


def _regex_rule(p21_rules_path, p21_domain_map_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)
    classified = classify_rules(p21_rules, [])
    return next(rule for rule in classified if rule.p21_rule_id == "SD0002")


def test_generate_regex_rule_folder_contains_rule_manifest_and_cases(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
):
    generated = generate_rule_folder(_regex_rule(p21_rules_path, p21_domain_map_path), tmp_path)

    assert generated.generated_rule_id == "P21PORT-SDTMIG-SD0002"
    rule_dir = tmp_path / generated.generated_rule_id
    assert (rule_dir / "rule.yml").exists()
    assert (rule_dir / "manifest.json").exists()
    assert (rule_dir / "expected_results.csv").exists()
    assert (rule_dir / "positive" / "01" / "data" / ".env").exists()
    assert (rule_dir / "negative" / "01" / "data" / ".env").exists()
    assert (rule_dir / "positive" / "01" / "results").is_dir()
    assert (rule_dir / "negative" / "01" / "results").is_dir()

    rule_yml = yaml.safe_load((rule_dir / "rule.yml").read_text(encoding="utf-8"))
    assert rule_yml["Core"]["Id"] == "P21PORT-SDTMIG-SD0002"
    assert rule_yml["Check"]["not_matches"]["name"] == "AEDTC"
    assert rule_yml["Check"]["not_matches"]["value"] == r"^\d{4}-\d{2}-\d{2}$"

    manifest = json.loads((rule_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["source_rule_id"] == "SD0002"
    assert manifest["expected_positive_issues"] == 0
    assert manifest["expected_negative_issues"] == 1

    with (rule_dir / "expected_results.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    assert rows[0]["case_type"] == "positive"
    assert rows[0]["expected_issue_count"] == "0"
    assert rows[1]["case_type"] == "negative"
    assert rows[1]["expected_issue_count"] == "1"


def test_generate_regex_test_data_has_matching_variable_metadata(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
):
    generated = generate_rule_folder(_regex_rule(p21_rules_path, p21_domain_map_path), tmp_path)
    data_dir = tmp_path / generated.generated_rule_id / "negative" / "01" / "data"

    with (data_dir / "_variables.csv").open(newline="", encoding="utf-8") as handle:
        variable_rows = list(csv.DictReader(handle))
    with (data_dir / "ae.csv").open(newline="", encoding="utf-8") as handle:
        dataset_reader = csv.DictReader(handle)
        dataset_columns = dataset_reader.fieldnames
        dataset_rows = list(dataset_reader)

    assert dataset_columns == [row["variable"] for row in variable_rows]
    assert dataset_rows[0]["AEDTC"] == "01JAN2020"
