import json
import os
import subprocess
import sys

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
