import json

from cdisc_rulekit.compare_results import (
    classification_counts,
    compare_generated_results,
    comparison_gate_ok,
    write_comparison_report,
)


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


def test_compare_generated_results_does_not_pass_multi_issue_case_on_one_matching_issue(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-UNIQUE"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables",
                f"negative,01,2,{rule_id},DM,1,USUBJID",
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
                "results": [
                    {
                        "rule_id": rule_id,
                        "execution_status": "failed",
                        "dataset": "DM",
                        "error_count": 2,
                        "errors": [
                            {"rule_id": rule_id, "dataset": "DM", "row": 1, "variables": ["USUBJID"]},
                            {"rule_id": rule_id, "dataset": "AE", "row": 99, "variables": ["BAD"]},
                        ],
                    },
                ],
            },
        ),
        encoding="utf-8",
    )

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    assert not result.ok
    assert result.rows[0]["status"] == "PARTIAL_STRUCTURAL_CHECK"


def test_compare_generated_results_matches_issue_index_schema_for_multi_issue_case(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-UNIQUE"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,issue_index,expected_issue_count,rule_id,dataset,row,variables,usubjid,seq",
                f"positive,01,,0,{rule_id},DM,,,,",
                f"negative,01,1,2,{rule_id},DM,1,USUBJID,,",
                f"negative,01,2,2,{rule_id},DM,2,USUBJID,,",
                "",
            ],
        ),
        encoding="utf-8",
    )
    for case_type, rows in {
        "positive": [],
        "negative": [
            {"rule_id": rule_id, "dataset": "DM", "row": 1, "variables": ["USUBJID"]},
            {"rule_id": rule_id, "dataset": "DM", "row": 2, "variables": ["USUBJID"]},
        ],
    }.items():
        actual_dir = tmp_path / "core_runs" / rule_id / case_type / "01"
        actual_dir.mkdir(parents=True)
        (actual_dir / "report.json").write_text(
            json.dumps(
                {
                    "results": [
                        {
                            "rule_id": rule_id,
                            "execution_status": "failed" if rows else "passed",
                            "dataset": "DM",
                            "error_count": len(rows),
                            "errors": rows,
                        },
                    ],
                },
            ),
            encoding="utf-8",
        )

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    assert result.ok
    assert result.pass_count == 2


def test_compare_generated_results_counts_duplicate_issue_signatures(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-DUPLICATE"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,issue_index,expected_issue_count,rule_id,dataset,row,variables,usubjid,seq",
                f"negative,01,1,3,{rule_id},DM,1,USUBJID,,",
                f"negative,01,2,3,{rule_id},DM,1,USUBJID,,",
                f"negative,01,3,3,{rule_id},DM,2,USUBJID,,",
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
                "results": [
                    {
                        "rule_id": rule_id,
                        "execution_status": "failed",
                        "dataset": "DM",
                        "error_count": 3,
                        "errors": [
                            {"rule_id": rule_id, "dataset": "DM", "row": 1, "variables": ["USUBJID"]},
                            {"rule_id": rule_id, "dataset": "DM", "row": 2, "variables": ["USUBJID"]},
                            {"rule_id": rule_id, "dataset": "DM", "row": 2, "variables": ["USUBJID"]},
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


def test_compare_generated_results_allows_expected_variables_subset_for_official_core_json(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1019-6683B90E"
    rule_dir = generated_root / rule_id
    rule_dir.mkdir(parents=True)
    (rule_dir / "expected_results.csv").write_text(
        "\n".join(
            [
                "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables",
                f"negative,01,1,{rule_id},SV,1,VISITDY|DOMAIN",
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
                        "dataset": "SV",
                        "USUBJID": "P21PORT-001",
                        "row": 1,
                        "SEQ": "",
                        "variables": ["SVUPDES", "VISITDY", "DOMAIN"],
                    },
                ],
                "Rules_Report": [{"core_id": rule_id, "status": "ISSUE REPORTED"}],
            },
        ),
        encoding="utf-8",
    )

    result = compare_generated_results(generated_root, tmp_path / "core_runs")
    strict_result = compare_generated_results(
        generated_root,
        tmp_path / "core_runs",
        strict_structure=True,
    )

    assert result.ok
    assert result.rows[0]["status"] == "PASS"
    assert not strict_result.ok
    assert strict_result.rows[0]["status"] == "FAIL"
    assert strict_result.rows[0]["notes"] == "structural issue fields did not match"


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


def test_compare_generated_results_fails_mixed_skipped_and_issues_even_when_skips_are_allowed(tmp_path):
    generated_root = tmp_path / "generated_rules"
    rule_id = "P21PORT-SDTMIG-SD1210-ABCDEF01"
    _expected_rule(generated_root, rule_id)
    actual_dir = tmp_path / "core_runs" / rule_id / "negative" / "01"
    actual_dir.mkdir(parents=True)
    (actual_dir / "report.json").write_text(
        json.dumps(
            {
                "Issue_Details": [
                    {
                        "core_id": rule_id,
                        "dataset": "DM",
                        "row": 1,
                        "variables": ["RFICDTC"],
                    },
                ],
                "Rules_Report": [
                    {"core_id": rule_id, "status": "SKIPPED"},
                    {"core_id": rule_id, "status": "ISSUE REPORTED"},
                ],
            },
        ),
        encoding="utf-8",
    )
    positive_dir = tmp_path / "core_runs" / rule_id / "positive" / "01"
    positive_dir.mkdir(parents=True)
    (positive_dir / "report.json").write_text(json.dumps({"summary": {"error_count": 0}, "results": []}), encoding="utf-8")

    result = compare_generated_results(generated_root, tmp_path / "core_runs")

    negative = next(row for row in result.rows if row["case_type"] == "negative")
    assert negative["status"] == "ACTUAL_MIXED_SKIPPED_AND_ISSUES"
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_compare_generated_results_empty_input_is_not_ok(tmp_path):
    result = compare_generated_results(tmp_path / "generated_rules", tmp_path / "core_runs")

    assert not result.ok
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_compare_generated_results_reports_empty_expected_results_per_rule(tmp_path):
    generated_root = tmp_path / "generated_rules"
    empty_rule = generated_root / "P21PORT-SDTMIG-EMPTY"
    empty_rule.mkdir(parents=True)
    (empty_rule / "expected_results.csv").write_text(
        "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables\n",
        encoding="utf-8",
    )
    _expected_rule(generated_root, "P21PORT-SDTMIG-PASS")
    actual_dir = tmp_path / "core_runs" / "P21PORT-SDTMIG-PASS" / "positive" / "01"
    actual_dir.mkdir(parents=True)
    (actual_dir / "report.json").write_text(json.dumps({"summary": {"error_count": 0}, "results": []}), encoding="utf-8")
    negative_dir = tmp_path / "core_runs" / "P21PORT-SDTMIG-PASS" / "negative" / "01"
    negative_dir.mkdir(parents=True)
    (negative_dir / "report.json").write_text(
        json.dumps(
            {
                "results": [
                    {
                        "rule_id": "P21PORT-SDTMIG-PASS",
                        "execution_status": "failed",
                        "dataset": "DM",
                        "error_count": 1,
                        "errors": [
                            {
                                "rule_id": "P21PORT-SDTMIG-PASS",
                                "dataset": "DM",
                                "row": 1,
                                "variables": ["RFICDTC"],
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
    empty = next(row for row in result.rows if row["generated_rule_id"] == "P21PORT-SDTMIG-EMPTY")
    assert empty["status"] == "EXPECTED_EMPTY"
    assert classification_counts(result)["EXPECTED_OUTPUT_EMPTY"] == 1


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
    assert (tmp_path / "reports" / "official_core_failure_classification.csv").exists()
    classification = json.loads((tmp_path / "reports" / "official_core_failure_classification.json").read_text(encoding="utf-8"))
    assert classification["supported_mismatch_rows"] == 0
    assert classification["coverage_gap_rows"] == 0
    assert classification["missing_output_rows"] == 2
