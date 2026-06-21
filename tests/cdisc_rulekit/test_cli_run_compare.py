import json
import os
import subprocess
import sys


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
        value = "2020-01-01" if case_type == "positive" else ""
        (data_dir / "dm.csv").write_text(f"STUDYID,DOMAIN,USUBJID,RFICDTC\nS1,DM,01,{value}\n", encoding="utf-8")
    (rule_dir / "rule.yml").write_text("Core:\n  Id: P21PORT-SDTMIG-SD1210-ABCDEF01\n", encoding="utf-8")
    (rule_dir / "manifest.json").write_text(json.dumps({"generated_rule_id": rule_id}), encoding="utf-8")
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables",
                f"positive,01,0,{rule_id},DM,,",
                f"negative,01,1,{rule_id},DM,1,RFICDTC",
                "",
            ],
        ),
        encoding="utf-8",
    )
    return rule_dir


def test_run_core_dry_run_and_compare_results_cli(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _generated_rule(generated_root, rule_id)
    out_dir = tmp_path / "output"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    run = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "run-core",
            "--generated-rules",
            str(generated_root),
            "--out",
            str(out_dir),
            "--dry-run",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "run-core dry-run complete: 2 cases planned" in run.stdout
    assert (out_dir / "reports" / "core_run_plan.json").exists()

    positive = out_dir / "core_runs" / rule_id / "positive" / "01"
    negative = out_dir / "core_runs" / rule_id / "negative" / "01"
    positive.mkdir(parents=True)
    negative.mkdir(parents=True)
    (positive / "report.json").write_text(json.dumps({"summary": {"error_count": 0}, "results": []}), encoding="utf-8")
    (negative / "report.json").write_text(
        json.dumps(
            {
                "summary": {"error_count": 1},
                "results": [
                    {
                        "rule_id": rule_id,
                        "error_count": 1,
                        "errors": [{"rule_id": rule_id, "dataset": "DM", "row": 1, "variables": ["RFICDTC"]}],
                    },
                ],
            },
        ),
        encoding="utf-8",
    )

    compare = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "compare-results",
            "--generated-rules",
            str(generated_root),
            "--actual-root",
            str(out_dir / "core_runs"),
            "--out",
            str(out_dir / "reports"),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "compare-results complete: ok" in compare.stdout
    assert (out_dir / "reports" / "comparison_summary.csv").exists()


def test_compare_results_cli_can_allow_actual_skipped_as_coverage_gap(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _generated_rule(generated_root, rule_id)
    actual_root = tmp_path / "core_runs"
    positive = actual_root / rule_id / "positive" / "01"
    negative = actual_root / rule_id / "negative" / "01"
    positive.mkdir(parents=True)
    negative.mkdir(parents=True)
    (positive / "report.json").write_text(json.dumps({"summary": {"error_count": 0}, "results": []}), encoding="utf-8")
    (negative / "report.json").write_text(
        json.dumps(
            {
                "Issue_Details": [],
                "Rules_Report": [{"core_id": rule_id, "status": "SKIPPED"}],
            },
        ),
        encoding="utf-8",
    )
    out_dir = tmp_path / "reports"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    compare = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "compare-results",
            "--generated-rules",
            str(generated_root),
            "--actual-root",
            str(actual_root),
            "--out",
            str(out_dir),
            "--allow-actual-skipped",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "compare-results complete: ok" in compare.stdout
    comparison = json.loads((out_dir / "comparison_summary.json").read_text(encoding="utf-8"))
    assert comparison["ok"] is False
    assert comparison["gate_ok"] is True
    assert comparison["allow_actual_skipped"] is True
    classification = json.loads((out_dir / "official_core_failure_classification.json").read_text(encoding="utf-8"))
    assert classification["supported_mismatch_rows"] == 0
    assert classification["coverage_gap_rows"] == 1


def test_run_core_executes_engine_command_and_writes_execution_summary(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _generated_rule(generated_root, rule_id)
    fake_engine = tmp_path / "fake_engine.py"
    fake_engine.write_text(
        "\n".join(
            [
                "import json",
                "import pathlib",
                "import sys",
                "args = sys.argv[1:]",
                "output = pathlib.Path(args[args.index('--output') + 1])",
                "output.mkdir(parents=True, exist_ok=True)",
                "dataset_paths = [args[index + 1] for index, arg in enumerate(args) if arg == '--dataset-path']",
                "assert all(path.endswith('.csv') for path in dataset_paths)",
                "assert not any(pathlib.Path(path).name.startswith('_') for path in dataset_paths)",
                "payload = {'summary': {'error_count': 0}, 'results': []}",
                "(output / 'report.json').write_text(json.dumps(payload), encoding='utf-8')",
                "(output / 'report.csv').write_text('rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq\\n', encoding='utf-8')",
                "print('fake engine ok')",
            ],
        ),
        encoding="utf-8",
    )
    out_dir = tmp_path / "output"
    env = os.environ.copy()
    env["PYTHONPATH"] = "src"

    run = subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "run-core",
            "--generated-rules",
            str(generated_root),
            "--out",
            str(out_dir),
            "--engine-command",
            f"{sys.executable} {fake_engine}",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    assert "run-core execution complete: ok, 2 passed, 0 failed" in run.stdout
    assert (out_dir / "reports" / "core_run_execution_summary.csv").exists()
    assert (out_dir / "core_runs" / rule_id / "positive" / "01" / "report.json").exists()
