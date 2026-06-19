import json
import os
import subprocess
import sys


def _generated_rule(root, rule_id="P21PORT-SDTMIG-SD1210-ABCDEF01"):
    rule_dir = root / rule_id
    (rule_dir / "positive" / "01" / "data").mkdir(parents=True)
    (rule_dir / "negative" / "01" / "data").mkdir(parents=True)
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
