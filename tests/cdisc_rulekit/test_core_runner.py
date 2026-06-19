import json

from cdisc_rulekit.core_runner import build_core_run_plan, write_core_run_plan


def _generated_rule(root, rule_id="P21PORT-SDTMIG-SD1210-ABCDEF01"):
    rule_dir = root / rule_id
    (rule_dir / "positive" / "01" / "data").mkdir(parents=True)
    (rule_dir / "negative" / "01" / "data").mkdir(parents=True)
    (rule_dir / "rule.yml").write_text("Core:\n  Id: P21PORT-SDTMIG-SD1210-ABCDEF01\n", encoding="utf-8")
    (rule_dir / "manifest.json").write_text(json.dumps({"generated_rule_id": rule_id}), encoding="utf-8")
    return rule_dir


def test_build_core_run_plan_creates_positive_and_negative_dry_run_commands(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)

    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command="cargo run -p core-cli -- validate",
        dry_run=True,
    )

    assert len(plan.items) == 2
    assert {item.case_type for item in plan.items} == {"positive", "negative"}
    first = plan.items[0]
    assert first.dry_run is True
    assert first.command[:5] == ["cargo", "run", "-p", "core-cli", "--"]
    assert "--local-rules" in first.command
    assert str(generated_root / first.generated_rule_id) in first.command
    assert "--dataset-path" in first.command
    assert str(generated_root / first.generated_rule_id / first.case_type / "01" / "data") in first.command


def test_write_core_run_plan_outputs_json_and_markdown(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    plan = build_core_run_plan(generated_root, run_root=tmp_path / "core_runs", dry_run=True)

    write_core_run_plan(tmp_path / "reports", plan)

    payload = json.loads((tmp_path / "reports" / "core_run_plan.json").read_text(encoding="utf-8"))
    assert payload["dry_run"] is True
    assert payload["case_count"] == 2
    assert (tmp_path / "reports" / "core_run_plan.md").exists()
