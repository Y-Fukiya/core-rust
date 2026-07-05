import csv
import json
import os
import subprocess
import sys
import zipfile


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
    assert statuses["SD0003"] == "AUTO_CONVERTIBLE"

    readiness = json.loads((out_dir / "reports" / "readiness_summary.json").read_text(encoding="utf-8"))
    assert readiness["total_p21_rules"] == 3
    assert readiness["generated_rules_created"] == 0


def test_convert_p21_config_writes_local_catalog_without_fetching(tmp_path):
    config = tmp_path / "p21-config.xml"
    config.write_text(
        """
<config version="2204.0" agency="FDA" name="SDTM-IG">
  <standard name="SDTM-IG" version="3.3">
    <rule id="SD1234" category="Validation" severity="Error" type="Required">
      <message>AE.AETERM must be populated. See CG1234.</message>
      <description>Adverse event term is required.</description>
      <domain>AE</domain>
      <class>EVENTS</class>
      <target>AETERM</target>
      <variable>AETERM</variable>
      <test>AETERM != ""</test>
    </rule>
  </standard>
</config>
""".strip(),
        encoding="utf-8",
    )
    out_dir = tmp_path / "catalog"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "convert-p21-config",
            "--input",
            str(config),
            "--source-label",
            "sdtm33",
            "--out",
            str(out_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "convert-p21-config complete: 1 rule(s)" in result.stdout
    with (out_dir / "p21_rules_normalized.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    assert len(rows) == 1
    assert rows[0]["p21_rule_id"] == "SD1234"
    assert rows[0]["standard_name"] == "SDTM-IG"
    assert rows[0]["standard_version"] == "3.3"
    assert rows[0]["domains"] == '["AE"]'
    assert rows[0]["variables"] == '["AETERM"]'
    assert json.loads(rows[0]["raw_condition"]) == {
        "target": "AETERM",
        "test": 'AETERM != ""',
        "variable": "AETERM",
    }
    assert rows[0]["source_path"] == "sdtm33"
    assert str(tmp_path) not in rows[0]["source_rule_key"]
    assert str(tmp_path) not in rows[0]["raw_record"]
    report = (out_dir / "extraction_report.md").read_text(encoding="utf-8")
    assert "does not download" in report
    assert "user-supplied" in report


def test_build_readonly_honors_standard_and_limit(
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
            "--limit",
            "1",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "build-readonly complete: 1 P21 rules" in result.stdout
    with (out_dir / "catalog" / "p21_rules_normalized.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    assert [row["p21_rule_id"] for row in rows] == ["SD0001"]


def test_build_readonly_accepts_open_rules_zip(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
    open_rules_repo_path,
):
    archive = tmp_path / "cdisc-open-rules-main.zip"
    with zipfile.ZipFile(archive, "w") as zip_handle:
        for path in open_rules_repo_path.rglob("*"):
            if path.is_file():
                zip_handle.write(path, path.relative_to(open_rules_repo_path.parent))
        zip_handle.writestr("open_rules/__pycache__/ignored.pyc", b"cache")
        zip_handle.writestr("open_rules/.pytest_cache/ignored", "cache")

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
            str(archive),
            "--out",
            str(out_dir),
            "--standard",
            "SDTM-IG",
            "--limit",
            "1",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "build-readonly complete: 1 P21 rules, 1 CORE rules" in result.stdout
    assert (out_dir / "_work" / "open_rules_zip" / "open_rules" / "Published" / "CORE-000001" / "rule.yml").exists()
    assert not (out_dir / "_work" / "open_rules_zip" / "open_rules" / "__pycache__").exists()
    assert not (out_dir / "_work" / "open_rules_zip" / "open_rules" / ".pytest_cache").exists()


def test_pilot_preflight_reports_input_readiness(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
    open_rules_repo_path,
):
    out_dir = tmp_path / "reports"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "pilot-preflight",
            "--p21-rules",
            str(p21_rules_path),
            "--p21-domain-map",
            str(p21_domain_map_path),
            "--open-rules-repo",
            str(open_rules_repo_path),
            "--out",
            str(out_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "pilot-preflight complete" in result.stdout
    report = json.loads((out_dir / "pilot_preflight.json").read_text(encoding="utf-8"))
    assert report["ok"] is True
    assert report["p21_rule_count"] == 4
    assert report["open_rules_published_rule_yml_count"] == 1


def test_build_readonly_filters_core_testdata_inventory_to_standard(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
):
    repo = tmp_path / "open_rules"
    sdtm_rule = repo / "Published" / "CORE-SDTM"
    adam_rule = repo / "Published" / "CORE-ADAM"
    (sdtm_rule / "positive" / "01" / "data").mkdir(parents=True)
    (adam_rule / "positive" / "01" / "data").mkdir(parents=True)
    (sdtm_rule / "positive" / "01" / "data" / "ae.csv").write_text("AETERM\nHEADACHE\n", encoding="utf-8")
    (adam_rule / "positive" / "01" / "data" / "adsl.csv").write_text("USUBJID\n01\n", encoding="utf-8")
    (sdtm_rule / "rule.yml").write_text(
        """
Core:
  Id: CORE-SDTM
Authorities:
  - Standards:
      - Name: SDTMIG
Check:
  not_empty:
    name: AETERM
Scope:
  Domains:
    Include: [AE]
""".strip()
        + "\n",
        encoding="utf-8",
    )
    (adam_rule / "rule.yml").write_text(
        """
Core:
  Id: CORE-ADAM
Authorities:
  - Standards:
      - Name: ADAMIG
Check:
  not_empty:
    name: USUBJID
Scope:
  Domains:
    Include: [ADSL]
""".strip()
        + "\n",
        encoding="utf-8",
    )
    out_dir = tmp_path / "output"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    subprocess.run(
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
            str(repo),
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

    with (out_dir / "catalog" / "core_testdata_inventory.csv").open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))
    assert [row["core_rule_id"] for row in rows] == ["CORE-SDTM"]
