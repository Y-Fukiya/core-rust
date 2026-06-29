import json
import sys

from cdisc_rulekit.core_runner import build_core_run_plan, execute_core_run_plan, write_core_run_plan


def _generated_rule(root, rule_id="P21PORT-SDTMIG-SD1210-ABCDEF01"):
    rule_dir = root / rule_id
    for case_type in ("positive", "negative"):
        data_dir = rule_dir / case_type / "01" / "data"
        data_dir.mkdir(parents=True)
        (data_dir / ".env").write_text("PRODUCT=SDTMIG\nVERSION=3-3\n", encoding="utf-8")
        (data_dir / "_datasets.csv").write_text("Filename,Label\ndm,Demographics\n", encoding="utf-8")
        (data_dir / "_variables.csv").write_text(
            "dataset,variable,label,type,length\ndm,DOMAIN,DOMAIN,Char,2\n",
            encoding="utf-8",
        )
        (data_dir / "dm.csv").write_text("STUDYID,DOMAIN,USUBJID,RFICDTC\nS1,DM,01,2020-01-01\n", encoding="utf-8")
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
    assert str(generated_root / first.generated_rule_id / "rule.yml") in first.command
    assert str(generated_root / first.generated_rule_id / "manifest.json") not in first.command
    assert "--dataset-path" in first.command
    assert str(generated_root / first.generated_rule_id / first.case_type / "01" / "data" / "dm.csv") in first.command
    assert str(generated_root / first.generated_rule_id / first.case_type / "01" / "data" / "_datasets.csv") not in first.command
    assert str(generated_root / first.generated_rule_id / first.case_type / "01" / "data" / "_variables.csv") not in first.command


def test_write_core_run_plan_outputs_json_and_markdown(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    plan = build_core_run_plan(generated_root, run_root=tmp_path / "core_runs", dry_run=True)

    write_core_run_plan(tmp_path / "reports", plan)

    payload = json.loads((tmp_path / "reports" / "core_run_plan.json").read_text(encoding="utf-8"))
    assert payload["dry_run"] is True
    assert payload["case_count"] == 2
    assert (tmp_path / "reports" / "core_run_plan.md").exists()


def test_build_core_run_plan_can_write_file_base_output_for_python_core(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)

    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command="python core.py validate -s SDTMIG -v 3.4 --output-format json",
        output_mode="file-base",
        data_mode="data-dir",
    )

    first = plan.items[0]
    assert first.command[first.command.index("--output") + 1] == str(
        tmp_path / "core_runs" / first.generated_rule_id / first.case_type / "01" / "report"
    )
    assert first.command[first.command.index("--data") + 1] == str(
        generated_root / first.generated_rule_id / first.case_type / "01" / "data"
    )
    assert "--dataset-path" not in first.command


def test_build_core_run_plan_substitutes_env_placeholders_in_engine_command(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)

    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command="python core.py validate -s {product} -v {version} --output-format json",
        output_mode="file-base",
        data_mode="data-dir",
    )

    first = plan.items[0]
    assert first.command[:7] == ["python", "core.py", "validate", "-s", "SDTMIG", "-v", "3.3"]
    assert "{version}" not in first.command


def test_build_core_run_plan_preserves_env_placeholder_as_single_argument(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_dir = _generated_rule(generated_root)
    for env_path in rule_dir.glob("*/01/data/.env"):
        env_path.write_text("PRODUCT=SDTMIG --unexpected-flag\nVERSION=3-3\n", encoding="utf-8")

    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command="python core.py validate -s {product} -v {version}",
        output_mode="file-base",
        data_mode="data-dir",
    )

    first = plan.items[0]
    assert first.command[first.command.index("-s") + 1] == "SDTMIG --unexpected-flag"
    assert "--unexpected-flag" not in first.command


def test_execute_core_run_plan_can_run_from_engine_cwd(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    engine_dir = tmp_path / "engine"
    engine_dir.mkdir()
    fake_engine = engine_dir / "fake_engine.py"
    fake_engine.write_text(
        "\n".join(
            [
                "import pathlib",
                "import sys",
                "assert pathlib.Path.cwd().name == 'engine'",
                "output = pathlib.Path(sys.argv[sys.argv.index('--output') + 1])",
                "output.mkdir(parents=True, exist_ok=True)",
                "(output / 'report.json').write_text('{}', encoding='utf-8')",
            ],
        ),
        encoding="utf-8",
    )
    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command=f"python {fake_engine}",
        dry_run=False,
    )

    result = execute_core_run_plan(plan, engine_cwd=engine_dir)

    assert result.ok


def test_execute_core_run_plan_supports_workers(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    fake_engine = tmp_path / "fake_engine.py"
    fake_engine.write_text(
        "\n".join(
            [
                "import pathlib",
                "import sys",
                "output = pathlib.Path(sys.argv[sys.argv.index('--output') + 1])",
                "output.mkdir(parents=True, exist_ok=True)",
                "(output / 'report.json').write_text('{}', encoding='utf-8')",
            ],
        ),
        encoding="utf-8",
    )
    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command=f"python {fake_engine}",
        dry_run=False,
    )

    result = execute_core_run_plan(plan, workers=2)

    assert result.ok
    assert result.pass_count == 2


def test_execute_core_run_plan_records_timeout_failures(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _generated_rule(generated_root)
    fake_engine = tmp_path / "fake_engine.py"
    fake_engine.write_text(
        "\n".join(
            [
                "import time",
                "time.sleep(5)",
            ],
        ),
        encoding="utf-8",
    )
    plan = build_core_run_plan(
        generated_root,
        run_root=tmp_path / "core_runs",
        engine_command=f"{sys.executable} {fake_engine}",
        dry_run=False,
    )

    result = execute_core_run_plan(plan, timeout_seconds=0.1)

    assert not result.ok
    assert result.fail_count == 2
    assert result.rows[0]["status"] == "FAIL"
    assert result.rows[0]["returncode"] == "TIMEOUT"
    assert "timed out after 0.1s" in result.rows[0]["stderr"]
