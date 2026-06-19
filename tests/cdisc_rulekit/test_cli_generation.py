import json
import os
import subprocess
import sys


def test_cli_generate_and_validate_structure(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
    open_rules_repo_path,
):
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
            str(open_rules_repo_path),
            "--out",
            str(out_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "generate",
            "--conversion-status",
            str(out_dir / "catalog" / "conversion_status.jsonl"),
            "--out",
            str(out_dir / "generated_rules"),
            "--status",
            "AUTO_CONVERTIBLE",
            "--limit",
            "5",
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )
    subprocess.run(
        [
            sys.executable,
            "-m",
            "cdisc_rulekit.cli",
            "validate-structure",
            "--generated",
            str(out_dir / "generated_rules"),
            "--out",
            str(out_dir / "reports"),
        ],
        check=True,
        capture_output=True,
        text=True,
        env=env,
    )

    generated_dir = out_dir / "generated_rules" / "P21PORT-SDTMIG-SD0002"
    assert (generated_dir / "rule.yml").exists()
    report = json.loads((out_dir / "reports" / "structure_validation.json").read_text(encoding="utf-8"))
    assert report["summary"]["failed"] == 0
