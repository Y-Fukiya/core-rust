import json

import pytest

from cdisc_rulekit.cli import main
from cdisc_rulekit.compare_results import (
    classification_counts,
    compare_generated_results,
    comparison_gate_ok,
)


RULE = "CORE-000001"
GENERATED = "generated-directory"


def _native(**changes):
    result = {"rule_id": RULE, "execution_status": "passed", "error_count": 0, "errors": []}
    result.update(changes)
    return {"results": [result]}


def _official(status="SUCCESS", **changes):
    result = {"core_id": RULE, "status": status}
    result.update(changes)
    return {"Rules_Report": [result], "Issue_Details": []}


@pytest.fixture(params=[False, True], ids=["legacy-row", "issue-index"])
def case(tmp_path, request):
    generated = tmp_path / "generated" / GENERATED
    generated.mkdir(parents=True)
    header = "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables"
    row = f"positive,01,0,{RULE},DM,,"
    if request.param:
        header += ",issue_index"
        row += ","
    (generated / "expected_results.csv").write_text(header + "\n" + row + "\n")
    actual = tmp_path / "actual" / GENERATED / "positive" / "01"
    actual.mkdir(parents=True)
    return generated.parent, actual


def _compare(case):
    generated, actual = case
    return compare_generated_results(generated, actual.parents[2])


@pytest.mark.parametrize("payload", [
    {}, [], None, 42, "report", {"results_data": []},
    {"datasets": [{"dataset": "DM", "execution_status": "success", "number_errors": 3,
                   "errors": [{"row": 1, "SEQ": "", "USUBJID": "01", "value": "", "validated": True}]}]},
    {"results": []}, {"results": None}, {"results": {}}, {"results": [None]},
    {"results": ["passed"]}, {"results": [{}]},
    _native(rule_id="OTHER"), _native(rule_id=None), _native(rule_id=""),
    _native(core_id="OTHER"), _native(execution_status="success"),
    _native(execution_status=""), _native(execution_status=None),
    _native(execution_status=[]), _native(execution_status="execution_error"),
    _native(execution_status="pending"), _native(execution_status="failed"),
    _native(error_count=-1), _native(error_count=True), _native(error_count=0.5),
    _native(error_count="0"), _native(error_count=None), _native(errors=None),
    _native(errors={}), _native(errors=[None]), _native(errors=[{}]),
    _native(dataset={}), _native(variables=[{}]), _native(row=True),
    _native(execution_status="failed", error_count=2, errors=[{}]),
    _native(execution_status="failed", error_count=1, errors=[{"rule_id": "OTHER"}]),
    _native(execution_status="failed", error_count=1, errors=[{"core_id": "OTHER"}]),
    _native(execution_status="skipped", error_count=1, errors=[{}]),
    {**_native(), "summary": {"error_count": 3}}, {**_native(), "summary": []},
    {**_native(), "datasets": []}, {**_native(), **_official()},
    {"Rules_Report": []}, {"Issue_Details": []},
    {"Rules_Report": [], "Issue_Details": []},
    {"Rules_Report": None, "Issue_Details": []},
    {"Rules_Report": [None], "Issue_Details": []},
    {"Rules_Report": [{"status": "SUCCESS"}], "Issue_Details": []},
    {**_official(), "Issue_Details": None},
    {**_official(), "Issue_Details": [None]},
    {**_official(), "Issue_Details": [{"core_id": RULE}]},
    {**_official("ISSUE REPORTED"), "Issue_Details": [{"core_id": "OTHER"}]},
    {**_official("ISSUE REPORTED"), "Issue_Details": [{}]},
    _official(core_id="OTHER"), _official(rule_id="OTHER"), _official("EXECUTION ERROR"),
    _official("PASSED"), _official("pending"), _official(None), _official([]),
    _official("ISSUE REPORTED"),
    {"Rules_Report": [{"core_id": RULE, "status": "SKIPPED"}, {"core_id": "OTHER", "status": "SUCCESS"}], "Issue_Details": []},
    {"results": [_native(execution_status="skipped")["results"][0], {}]},
])
def test_invalid_json_never_passes_or_becomes_an_allowed_skip(case, payload):
    (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    row = result.rows[0]
    assert row["status"] == "ACTUAL_INVALID_REPORT"
    assert row["actual_issue_count"] == ""
    assert row["notes"]
    assert not comparison_gate_ok(result, allow_actual_skipped=True)
    assert classification_counts(result) == {"ACTUAL_INVALID_REPORT": 1}


@pytest.mark.parametrize("payload", [_native(), _official()])
def test_valid_positive_requires_effective_manifest_rule_identity(case, payload):
    (case[1] / "report.json").write_text(json.dumps(payload))
    assert _compare(case).ok


@pytest.mark.parametrize("payload", [_native(), _official()])
def test_directory_identity_fallback(case, payload):
    expected = case[0] / GENERATED / "expected_results.csv"
    expected.write_text(expected.read_text().replace(RULE, ""))
    (case[1] / "report.json").write_text(json.dumps(payload).replace(RULE, GENERATED))
    assert _compare(case).ok


@pytest.mark.parametrize("content", ["{", "null", "{}", '{"results": []}', '{"results": NaN}'])
def test_invalid_json_is_authoritative_even_with_valid_csv(case, content):
    (case[1] / "report.json").write_text(content)
    (case[1] / "report.csv").write_text(f"rule_id,execution_status,error_count\n{RULE},passed,0\n")
    assert _compare(case).rows[0]["status"] == "ACTUAL_INVALID_REPORT"


@pytest.mark.parametrize("filename", ["report.json", "report.csv"])
def test_invalid_encoding_and_directory_instead_of_report(case, filename):
    path = case[1] / filename
    path.write_bytes(b"\xff")
    assert _compare(case).rows[0]["status"] == "ACTUAL_INVALID_REPORT"
    path.unlink()
    path.mkdir()
    assert _compare(case).rows[0]["status"] == "ACTUAL_INVALID_REPORT"


@pytest.mark.parametrize("content", [
    "", "unrelated\n0\n", "rule_id,execution_status,error_count\n",
    f"rule_id,execution_status,error_count,error_count\n{RULE},passed,0,0\n",
    f"rule_id,execution_status,error_count,\n{RULE},passed,0,\n",
    f"rule_id,execution_status,error_count\n{RULE},passed\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,0,extra\n",
    f'rule_id,execution_status,error_count\n{RULE},passed,"0\n',
    "rule_id,execution_status,error_count\nOTHER,passed,0\n",
    f"rule_id,execution_status,error_count\n{RULE},,0\n",
    f"rule_id,execution_status,error_count\n{RULE},pending,0\n",
    f"rule_id,execution_status,error_count\n{RULE},execution_error,0\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,-1\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,0.0\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,False\n",
    f"rule_id,execution_status,error_count\n{RULE},passed,1\n",
    f"rule_id,execution_status,error_count\n{RULE},failed,0\n",
    f"rule_id,execution_status,error_count\n{RULE},failed,2\n",
    f"rule_id,execution_status,error_count\n{RULE},skipped,1\n",
    f"rule_id,execution_status,error_count,row\n{RULE},skipped,0,1\n",
    f"rule_id,execution_status,error_count\n{RULE},skipped,0\nOTHER,passed,0\n",
])
def test_invalid_csv_is_nonpass(case, content):
    (case[1] / "report.csv").write_text(content)
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_INVALID_REPORT"
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_csv_positive(case):
    (case[1] / "report.csv").write_text(f"rule_id,execution_status,error_count\n{RULE},passed,0\n")
    assert _compare(case).ok


@pytest.mark.parametrize("format", ["native", "official", "csv"])
def test_valid_skip_is_only_waived_explicitly(case, format):
    if format == "csv":
        (case[1] / "report.csv").write_text(f"rule_id,execution_status,error_count\n{RULE},skipped,0\n")
    else:
        payload = _native(execution_status="skipped") if format == "native" else _official("SKIPPED")
        (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_SKIPPED"
    assert not comparison_gate_ok(result)
    assert comparison_gate_ok(result, allow_actual_skipped=True)


@pytest.mark.parametrize("format", ["native", "official", "csv"])
def test_mixed_skips_and_findings_remain_hard_failure(case, format):
    error = {"rule_id": RULE, "dataset": "DM", "row": 1}
    if format == "native":
        payload = {"results": [
            _native(execution_status="skipped")["results"][0],
            _native(execution_status="failed", error_count=1, errors=[error])["results"][0],
        ]}
    elif format == "official":
        payload = {"Rules_Report": [
            {"core_id": RULE, "status": "SKIPPED"}, {"core_id": RULE, "status": "ISSUE REPORTED"},
        ], "Issue_Details": [{"core_id": RULE, "dataset": "DM", "row": 1}]}
    else:
        (case[1] / "report.csv").write_text(
            f"rule_id,execution_status,error_count\n{RULE},skipped,0\n{RULE},failed,1\n"
        )
    if format != "csv":
        (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_MIXED_SKIPPED_AND_ISSUES"
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_csv_repeated_aggregate_counts_are_not_summed(case):
    expected = case[0] / GENERATED / "expected_results.csv"
    expected.write_text(
        "case_type,case_id,issue_index,expected_issue_count,rule_id,dataset,row,variables\n"
        f"positive,01,1,2,{RULE},DM,1,USUBJID\n"
        f"positive,01,2,2,{RULE},DM,2,USUBJID\n"
    )
    (case[1] / "report.csv").write_text(
        "rule_id,execution_status,error_count,dataset,row,variables\n"
        f"{RULE},failed,2,DM,1,USUBJID\n{RULE},failed,2,DM,2,USUBJID\n"
    )
    result = _compare(case)
    assert result.ok
    assert result.rows[0]["actual_issue_count"] == 2


@pytest.mark.parametrize("summary", [
    {"total_results": 2, "passed": 1, "failed": 0, "skipped": 1, "error_count": 0},
    {"total_results": 0}, {"passed": 0}, {"failed": 1}, {"skipped": 1}, {"error_count": 1},
    {"total_results": 1, "passed": 0, "failed": 0, "skipped": 1, "error_count": 0},
])
def test_native_summary_must_match_parsed_executions(case, summary):
    (case[1] / "report.json").write_text(json.dumps({**_native(), "summary": summary}))
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_INVALID_REPORT"
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


@pytest.mark.parametrize("field", ["total_results", "passed", "failed", "skipped", "error_count"])
@pytest.mark.parametrize("value", [-1, True, False, "0", "1", 0.0, 1.0, None, [], {}])
def test_native_summary_counters_require_nonnegative_integers(case, field, value):
    payload = {**_native(), "summary": {field: value}}
    (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_INVALID_REPORT"
    assert not comparison_gate_ok(result, allow_actual_skipped=True)


@pytest.mark.parametrize("summary", [
    {}, {"total_results": 1}, {"passed": 1}, {"failed": 0}, {"skipped": 0}, {"error_count": 0},
    {"total_results": 1, "passed": 1, "failed": 0, "skipped": 0, "error_count": 0},
])
def test_native_partial_and_full_summaries_remain_supported(case, summary):
    (case[1] / "report.json").write_text(json.dumps({**_native(), "summary": summary}))
    assert _compare(case).ok


def _expect_issue_rows(case, issues):
    expected = case[0] / GENERATED / "expected_results.csv"
    indexed = "issue_index" in expected.read_text().splitlines()[0].split(",")
    header = "case_type,case_id,expected_issue_count,rule_id,dataset,row,variables"
    if indexed:
        header += ",issue_index"
    rows = []
    for index, issue in enumerate(issues if indexed else issues[:1], start=1):
        row = f"positive,01,{len(issues)},{RULE},{issue['dataset']},{issue['row']},USUBJID"
        rows.append(row + (f",{index}" if indexed else ""))
    expected.write_text(header + "\n" + "\n".join(rows) + "\n")
    return "PASS" if indexed or len(issues) == 1 else "PARTIAL_STRUCTURAL_CHECK"


@pytest.mark.parametrize("skip", [False, True])
def test_native_summary_preserves_multiple_executions_and_datasets(case, skip):
    issues = [
        {"dataset": "DM", "row": 1, "variables": ["USUBJID"]},
        {"dataset": "AE", "row": 2, "variables": ["USUBJID"]},
    ]
    status = _expect_issue_rows(case, issues)
    payload = {"results": [
        _native(dataset="DM")["results"][0],
        _native(dataset="AE")["results"][0],
        _native(dataset="DM", execution_status="failed", error_count=2, errors=issues)["results"][0],
    ], "summary": {"total_results": 3 + int(skip), "passed": 2, "failed": 1,
                   "skipped": int(skip), "error_count": 2}}
    if skip:
        payload["results"].append(_native(dataset="VS", execution_status="skipped")["results"][0])
        status = "ACTUAL_MIXED_SKIPPED_AND_ISSUES"
    (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    assert result.rows[0]["status"] == status
    assert result.rows[0]["actual_issue_count"] == 2
    if skip:
        assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_native_summary_preserves_skip_waiver(case):
    payload = {"results": [
        _native(dataset="DM")["results"][0],
        _native(dataset="AE", execution_status="skipped")["results"][0],
    ], "summary": {"total_results": 2, "passed": 1, "failed": 0, "skipped": 1, "error_count": 0}}
    (case[1] / "report.json").write_text(json.dumps(payload))
    result = _compare(case)
    assert result.rows[0]["status"] == "ACTUAL_SKIPPED"
    assert not comparison_gate_ok(result)
    assert comparison_gate_ok(result, allow_actual_skipped=True)


@pytest.mark.parametrize(("counts", "representable"), [
    ([2, 2], True), ([1, 1], True), ([2, 1], False), ([2, 2, 2], False),
    ([2, 2, 1], True), ([2, 2, 2, 2], True), ([3, 3, 3, 1], True),
    ([3, 3, 2, 2, 1], False),
])
@pytest.mark.parametrize("skip", [False, True])
def test_csv_aggregate_count_distribution_representability(case, counts, representable, skip):
    issues = [{"dataset": "DM" if index % 2 else "AE", "row": index}
              for index in range(1, len(counts) + 1)]
    status = _expect_issue_rows(case, issues)
    content = "rule_id,execution_status,error_count,dataset,row,variables\n"
    content += f"{RULE},passed,0,VS,,\n"
    for count, issue in zip(counts, issues):
        content += f"{RULE},failed,{count},{issue['dataset']},{issue['row']},USUBJID\n"
    if skip:
        content += f"{RULE},skipped,0,VS,,\n"
        status = "ACTUAL_MIXED_SKIPPED_AND_ISSUES"
    if not representable:
        status = "ACTUAL_INVALID_REPORT"
    (case[1] / "report.csv").write_text(content)
    result = _compare(case)
    assert result.rows[0]["status"] == status
    assert result.rows[0]["actual_issue_count"] == (len(counts) if representable else "")
    if skip or not representable:
        assert not comparison_gate_ok(result, allow_actual_skipped=True)


def test_invalid_report_cli_and_classification_are_not_coverage_gaps(case, tmp_path):
    (case[1] / "report.json").write_text('{"datasets": []}')
    out = tmp_path / "reports"
    assert main([
        "compare-results", "--generated-rules", str(case[0]),
        "--actual-root", str(case[1].parents[2]), "--out", str(out), "--allow-actual-skipped",
    ]) == 1
    summary = json.loads((out / "official_core_failure_classification.json").read_text())
    assert summary["supported_mismatch_rows"] == 0
    assert summary["coverage_gap_rows"] == 0
    assert summary["classifications"][0]["classification"] == "ACTUAL_INVALID_REPORT"


def test_missing_actual_preserved(case):
    assert _compare(case).rows[0]["status"] == "ACTUAL_MISSING"


def test_grouped_expected_identity_conflict_is_nonpass(case):
    expected = case[0] / GENERATED / "expected_results.csv"
    expected.write_text(
        "case_type,case_id,issue_index,expected_issue_count,rule_id\n"
        f"positive,01,1,2,{RULE}\npositive,01,2,2,OTHER\n"
    )
    (case[1] / "report.json").write_text(json.dumps(_native()))
    assert _compare(case).rows[0]["status"] == "EXPECTED_INVALID"
