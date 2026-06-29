import json
import os
import subprocess
import sys
import pytest

from cdisc_rulekit.export_rules import export_generated_rules


def _generated_rule(root, rule_id="P21PORT-SDTMIG-SD1210-ABCDEF01", description="first"):
    rule_dir = root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "rule.yml").write_text(
        f"Core:\n  Id: {rule_id}\n  Description: {description}\n",
        encoding="utf-8",
    )
    (rule_dir / "manifest.json").write_text(json.dumps({"generated_rule_id": rule_id}), encoding="utf-8")
    return rule_dir


def test_export_generated_rules_copies_to_unpublished_new_rule_without_overwrite(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    source_rule = _generated_rule(generated_root, rule_id)
    open_rules_repo = tmp_path / "cdisc-open-rules"

    first = export_generated_rules(generated_root, open_rules_repo)

    target_rule = open_rules_repo / "Unpublished" / "NEW-RULE" / rule_id
    assert first.exported_count == 1
    assert (target_rule / "rule.yml").read_text(encoding="utf-8") == (source_rule / "rule.yml").read_text(encoding="utf-8")
    manifest = json.loads((open_rules_repo / "Unpublished" / "NEW-RULE" / "export_manifest.json").read_text(encoding="utf-8"))
    assert manifest["exported_count"] == 1

    (source_rule / "rule.yml").write_text(
        f"Core:\n  Id: {rule_id}\n  Description: changed\n",
        encoding="utf-8",
    )
    second = export_generated_rules(generated_root, open_rules_repo)

    assert second.skipped_count == 1
    assert "Description: first" in (target_rule / "rule.yml").read_text(encoding="utf-8")
    assert second.rows[0]["skip_reason"] == "TARGET_EXISTS"


def test_export_generated_rules_can_filter_to_comparison_passed_rules(tmp_path):
    generated_root = tmp_path / "generated_rules"
    passed_rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    skipped_rule_id = "P21PORT-SDTMIG-SD1234-3E0AC03D"
    _generated_rule(generated_root, passed_rule_id)
    _generated_rule(generated_root, skipped_rule_id)
    comparison = tmp_path / "comparison_summary.csv"
    comparison.write_text(
        "\n".join(
            [
                "generated_rule_id,case_type,case_id,expected_issue_count,actual_issue_count,status,rule_id,dataset,row,variables,usubjid,seq,notes",
                f"{passed_rule_id},positive,01,0,0,PASS,{passed_rule_id},DM,,,,,",
                f"{passed_rule_id},negative,01,1,1,PASS,{passed_rule_id},DM,1,RFICDTC,,,",
                f"{skipped_rule_id},positive,01,0,0,ACTUAL_SKIPPED,{skipped_rule_id},DI,,,,,actual CORE output contains skipped result(s)",
                f"{skipped_rule_id},negative,01,1,0,ACTUAL_SKIPPED,{skipped_rule_id},DI,1,DOMAIN,,,actual CORE output contains skipped result(s)",
                "",
            ],
        ),
        encoding="utf-8",
    )
    open_rules_repo = tmp_path / "cdisc-open-rules"

    summary = export_generated_rules(
        generated_root,
        open_rules_repo,
        comparison_summary=comparison,
        only_passed=True,
    )

    target_root = open_rules_repo / "Unpublished" / "NEW-RULE"
    assert summary.exported_count == 1
    assert summary.skipped_count == 1
    assert (target_root / passed_rule_id / "rule.yml").exists()
    assert not (target_root / skipped_rule_id).exists()
    skipped = next(row for row in summary.rows if row["generated_rule_id"] == skipped_rule_id)
    assert skipped["skip_reason"] == "COMPARISON_NOT_PASS"


def test_export_generated_rules_does_not_filter_without_only_passed(tmp_path):
    generated_root = tmp_path / "generated_rules"
    passed_rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    skipped_rule_id = "P21PORT-SDTMIG-SD1234-3E0AC03D"
    _generated_rule(generated_root, passed_rule_id)
    _generated_rule(generated_root, skipped_rule_id)
    comparison = tmp_path / "comparison_summary.csv"
    comparison.write_text(
        "\n".join(
            [
                "generated_rule_id,case_type,case_id,expected_issue_count,actual_issue_count,status,rule_id,dataset,row,variables,usubjid,seq,notes",
                f"{passed_rule_id},positive,01,0,0,PASS,{passed_rule_id},DM,,,,,",
                f"{skipped_rule_id},positive,01,0,0,ACTUAL_SKIPPED,{skipped_rule_id},DI,,,,,actual CORE output contains skipped result(s)",
                "",
            ],
        ),
        encoding="utf-8",
    )

    summary = export_generated_rules(
        generated_root,
        tmp_path / "cdisc-open-rules",
        comparison_summary=comparison,
    )

    assert summary.exported_count == 2
    assert summary.skipped_count == 0


def test_export_generated_rules_rejects_target_subdir_outside_open_rules_repo(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    outside = tmp_path / "outside"
    outside.mkdir()
    protected = outside / "P21PORT-SDTMIG-SD1210-ABCDEF01"
    protected.mkdir()
    (protected / "keep.txt").write_text("do not delete", encoding="utf-8")

    with pytest.raises(ValueError, match="inside open_rules_repo"):
        export_generated_rules(
            generated_root,
            tmp_path / "cdisc-open-rules",
            target_subdir="../outside",
            overwrite=True,
        )

    assert (protected / "keep.txt").exists()


def test_export_rules_cli_supports_overwrite(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _generated_rule(generated_root, rule_id, description="replacement")
    open_rules_repo = tmp_path / "cdisc-open-rules"
    existing = open_rules_repo / "Unpublished" / "NEW-RULE" / rule_id
    existing.mkdir(parents=True)
    (existing / "rule.yml").write_text("Core:\n  Id: stale\n", encoding="utf-8")
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    run = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "export-rules",
            "--generated-rules",
            str(generated_root),
            "--open-rules-repo",
            str(open_rules_repo),
            "--overwrite",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "export-rules complete: 1 exported, 0 skipped" in run.stdout
    assert "Description: replacement" in (existing / "rule.yml").read_text(encoding="utf-8")
