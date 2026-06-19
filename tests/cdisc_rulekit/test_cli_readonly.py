import csv
import json
import os
import subprocess
import sys


def test_build_readonly_writes_catalog_mapping_and_reports(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
    open_rules_repo_path,
):
    out_dir = tmp_path / "output"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "build-readonly",
            "--p21-rules",
            str(p21_rules_path),
            "--p21-domain-map",
            str(p21_domain_map_path),
            "--open-rules-repo",
            str(open_rules_repo_path),
            "--out",
            str(out_dir),
            "--standard",
            "SDTM-IG",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "build-readonly complete" in result.stdout
    expected_files = [
        "catalog/p21_rules_normalized.csv",
        "catalog/p21_rules_normalized.jsonl",
        "catalog/core_rules_normalized.csv",
        "catalog/core_rules_normalized.jsonl",
        "catalog/core_testdata_inventory.csv",
        "catalog/core_operator_inventory.csv",
        "catalog/core_operator_inventory.jsonl",
        "catalog/conversion_status.csv",
        "mapping/p21_to_core_mapping.csv",
        "mapping/p21_to_core_mapping.jsonl",
        "reports/conversion_status_summary.md",
        "reports/readiness_summary.json",
    ]
    for relative in expected_files:
        assert (out_dir / relative).exists(), relative
    assert not (out_dir / "generated_rules").exists()

    with (out_dir / "catalog" / "conversion_status.csv").open(newline="", encoding="utf-8") as handle:
        statuses = {row["p21_rule_id"]: row["conversion_status"] for row in csv.DictReader(handle)}
    assert statuses["SD0001"] == "NATIVE_CORE"
    assert statuses["SD0002"] == "AUTO_CONVERTIBLE"

    readiness = json.loads((out_dir / "reports" / "readiness_summary.json").read_text(encoding="utf-8"))
    assert readiness["total_p21_rules"] == 3
    assert readiness["generated_rules_created"] == 0
