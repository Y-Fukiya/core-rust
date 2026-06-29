import csv
import json
import os
import subprocess
import sys


def test_generate_and_validate_structure_cli(tmp_path):
    catalog = tmp_path / "p21_rules_normalized.jsonl"
    status = tmp_path / "conversion_status.csv"
    operators = tmp_path / "core_operator_inventory.csv"
    out_dir = tmp_path / "output"

    rule = {
        "source": "P21",
        "source_rule_id": "SD1210",
        "source_rule_key": "2204.0|FDA|SDTM-IG|SDTM-IG|3.3|SD1210|sdtmig.xml",
        "p21_rule_id": "SD1210",
        "standard_name": "SDTM-IG",
        "standard_version": "3.3",
        "agency": "FDA",
        "p21_rule_type": "Required",
        "message": "Missing value for RFICDTC",
        "description": "RFICDTC must be populated.",
        "domains": ["DM"],
        "variables": ["RFICDTC"],
        "target": "RFICDTC",
        "raw_condition": {},
        "raw_record": {},
    }
    catalog.write_text(json.dumps(rule, sort_keys=True) + "\n", encoding="utf-8")

    with status.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "source_rule_key",
                "conversion_status",
                "conversion_confidence",
                "conversion_reasons",
                "core_rule_id",
            ],
        )
        writer.writeheader()
        writer.writerow(
            {
                "source_rule_key": rule["source_rule_key"],
                "conversion_status": "AUTO_CONVERTIBLE",
                "conversion_confidence": "0.7",
                "conversion_reasons": json.dumps(["NO_CORE_MAPPING", "NO_CORE_MAPPING"]),
                "core_rule_id": "",
            },
        )

    with operators.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["operator", "raw_keys"])
        writer.writeheader()
        writer.writerow({"operator": "all", "raw_keys": "[]"})
        writer.writerow({"operator": "operator", "raw_keys": json.dumps(["name", "operator", "value"])})
        writer.writerow({"operator": "equal_to", "raw_keys": "[]"})
        writer.writerow({"operator": "empty", "raw_keys": "[]"})
        writer.writerow({"operator": "non_empty", "raw_keys": "[]"})

    env = os.environ.copy()
    env["PYTHONPATH"] = "src"
    generate = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "generate",
            "--p21-catalog",
            str(catalog),
            "--conversion-status",
            str(status),
            "--operator-inventory",
            str(operators),
            "--out",
            str(out_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "generate complete: 1 generated, 0 skipped" in generate.stdout
    assert (out_dir / "generated_rules").exists()
    assert (out_dir / "reports" / "generation_summary.csv").exists()

    validate = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "validate-structure",
            "--generated-rules",
            str(out_dir / "generated_rules"),
            "--out",
            str(out_dir / "reports"),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "validate-structure complete: ok" in validate.stdout
    assert (out_dir / "reports" / "structure_validation.md").exists()


def test_generate_cli_can_include_fuzzy_candidates(tmp_path):
    catalog = tmp_path / "p21_rules_normalized.jsonl"
    status = tmp_path / "conversion_status.csv"
    operators = tmp_path / "core_operator_inventory.csv"
    out_dir = tmp_path / "output"

    rule = {
        "source": "P21",
        "source_rule_id": "SD0087",
        "source_rule_key": "fuzzy-key",
        "p21_rule_id": "SD0087",
        "standard_name": "SDTM-IG",
        "standard_version": "3.3",
        "p21_rule_type": "Required",
        "domains": ["DM"],
        "variables": ["RFSTDTC"],
        "target": "RFSTDTC",
        "raw_condition": {},
        "raw_record": {},
    }
    catalog.write_text(json.dumps(rule, sort_keys=True) + "\n", encoding="utf-8")

    with status.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "source_rule_key",
                "conversion_status",
                "conversion_confidence",
                "conversion_reasons",
                "core_rule_id",
            ],
        )
        writer.writeheader()
        writer.writerow(
            {
                "source_rule_key": rule["source_rule_key"],
                "conversion_status": "AUTO_CONVERTIBLE",
                "conversion_confidence": "0.7",
                "conversion_reasons": json.dumps(["FUZZY_CORE_CANDIDATE", "NO_CORE_MAPPING"]),
                "core_rule_id": "",
            },
        )

    with operators.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=["operator", "raw_keys"])
        writer.writeheader()
        writer.writerow({"operator": "all", "raw_keys": "[]"})
        writer.writerow({"operator": "equal_to", "raw_keys": "[]"})
        writer.writerow({"operator": "empty", "raw_keys": "[]"})

    env = os.environ.copy()
    env["PYTHONPATH"] = "src"
    generate = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "generate",
            "--p21-catalog",
            str(catalog),
            "--conversion-status",
            str(status),
            "--operator-inventory",
            str(operators),
            "--out",
            str(out_dir),
            "--include-fuzzy-candidates",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "generate complete: 1 generated, 0 skipped" in generate.stdout
    generated_dir = next((out_dir / "generated_rules").iterdir())
    manifest = json.loads((generated_dir / "manifest.json").read_text(encoding="utf-8"))
    assert manifest["warnings"] == ["FUZZY_CORE_CANDIDATE_REQUIRES_REVIEW"]
