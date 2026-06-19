import json

from cdisc_rulekit.compare_results import compare_generated_results, write_comparison_report


def _expected_rule(root, rule_id="P21PORT-SDTMIG-SD1210-ABCDEF01"):
    rule_dir = root / rule_id
    rule_dir.mkdir(parents=True)
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


def test_compare_generated_results_matches_structural_fields_and_ignores_message(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _expected_rule(generated_root, rule_id)
    actual_root = tmp_path / "core_runs"
    positive = actual_root / rule_id / "positive" / "01"
    negative = actual_root / rule_id / "negative" / "01"
    positive.mkdir(parents=True)
    negative.mkdir(parents=True)
    (positive / "report.json").write_text(
        json.dumps({"summary": {"error_count": 0}, "results": []}),
        encoding="utf-8",
    )
    (negative / "report.json").write_text(
        json.dumps(
            {
                "summary": {"error_count": 1},
                "results": [
                    {
                        "rule_id": rule_id,
                        "execution_status": "failed",
                        "dataset": "DM",
                        "domain": "DM",
                        "error_count": 1,
                        "errors": [
                            {
                                "rule_id": rule_id,
                                "dataset": "DM",
                                "domain": "DM",
                                "row": 1,
                                "variables": ["RFICDTC"],
                                "message": "Different engine wording is okay",
                            },
                        ],
                    },
                ],
            },
        ),
        encoding="utf-8",
    )

    result = compare_generated_results(generated_root, actual_root)

    assert result.ok
    assert result.pass_count == 2
    assert result.fail_count == 0
    assert {row["status"] for row in result.rows} == {"PASS"}


def test_compare_generated_results_matches_optional_usubjid_and_seq_when_expected(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables,usubjid,seq",
                f"negative,01,1,{rule_id},DM,1,RFICDTC,SUBJ-001,2",
                "",
            ],
        ),
        encoding="utf-8",
    )
    actual_dir = tmp_path / "core_runs" / rule_id / "negative" / "01"
    actual_dir.mkdir(parents=True)
    (actual_dir / "report.json").write_text(
        json.dumps(
            {
                "summary": {"error_count": 1},
                "results": [
                    {
                        "rule_id": rule_id,
                        "execution_status": "failed",
                        "dataset": "DM",
                        "domain": "DM",
                        "error_count": 1,
                        "errors": [
                            {
                                "rule_id": rule_id,
                                "dataset": "DM",
                                "domain": "DM",
                                "row": 1,
                                "variables": ["RFICDTC"],
                                "message": "Different engine wording is okay",
                                "usubjid": "SUBJ-001",
                                "seq": "9",
                            },
                        ],
                    },
                ],
            },
        ),
        encoding="utf-8",
    )

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    assert not result.ok
    assert result.rows[0]["status"] == "FAIL"
    assert result.rows[0]["notes"] == "structural issue fields did not match"


def test_compare_generated_results_reads_official_core_json_report(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD0087-04B4C7DE"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables,usubjid,seq",
                f"negative,01,1,{rule_id},DM,1,RFSTDTC|DOMAIN,P21PORT-001,",
                "",
            ],
        ),
        encoding="utf-8",
    )
    actual_dir = tmp_path / "core_runs" / rule_id / "negative" / "01"
    actual_dir.mkdir(parents=True)
    (actual_dir / "report.json").write_text(
        json.dumps(
            {
                "Issue_Details": [
                    {
                        "core_id": rule_id,
                        "message": "Official CORE wording is not a comparison key",
                        "dataset": "DM",
                        "USUBJID": "P21PORT-001",
                        "row": 1,
                        "SEQ": "",
                        "variables": ["RFSTDTC", "DOMAIN"],
                    },
                ],
                "Rules_Report": [{"core_id": rule_id, "status": "ISSUE REPORTED"}],
            },
        ),
        encoding="utf-8",
    )

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    assert result.ok
    assert result.rows[0]["status"] == "PASS"


def test_compare_generated_results_reports_actual_skipped_separately_from_failure(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _expected_rule(generated_root, rule_id)
    actual_dir = tmp_path / "core_runs" / rule_id / "negative" / "01"
    actual_dir.mkdir(parents=True)
    (actual_dir / "report.csv").write_text(
        "\n".join(
            [
                "rule_id,execution_status,dataset,domain,row,variables,message,error_count,skipped_reason,usubjid,seq",
                f"{rule_id},skipped,DM,DM,,,unsupported,0,unsupported_operator,,",
                "",
            ],
        ),
        encoding="utf-8",
    )
    positive_dir = tmp_path / "core_runs" / rule_id / "positive" / "01"
    positive_dir.mkdir(parents=True)
    (positive_dir / "report.json").write_text(json.dumps({"summary": {"error_count": 0}, "results": []}), encoding="utf-8")

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    negative = next(row for row in result.rows if row["case_type"] == "negative")
    assert negative["status"] == "ACTUAL_SKIPPED"
    assert negative["notes"] == "actual CORE output contains skipped result(s)"


def test_compare_generated_results_reports_missing_actual_output(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _expected_rule(generated_root)

    result = compare_generated_results(generated_root, tmp_path / "missing_runs")

    assert not result.ok
    assert result.fail_count == 2
    assert {row["status"] for row in result.rows} == {"ACTUAL_MISSING"}


def test_write_comparison_report_outputs_csv_json_and_markdown(tmp_path):
    generated_root = tmp_path / "generated_rules"
    _expected_rule(generated_root)
    result = compare_generated_results(generated_root, tmp_path / "missing_runs")

    write_comparison_report(tmp_path / "reports", result)

    assert (tmp_path / "reports" / "comparison_summary.csv").exists()
    assert (tmp_path / "reports" / "comparison_summary.json").exists()
    assert (tmp_path / "reports" / "comparison_summary.md").exists()
