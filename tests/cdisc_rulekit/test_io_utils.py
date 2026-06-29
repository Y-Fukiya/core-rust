import csv

from cdisc_rulekit.io_utils import write_csv


def test_write_csv_escapes_spreadsheet_formula_prefixes(tmp_path):
    path = tmp_path / "report.csv"

    write_csv(
        path,
        [
            {"name": "=cmd|' /C calc'!A0"},
            {"name": "+SUM(1,1)"},
            {"name": "-1+2"},
            {"name": "@HYPERLINK(\"https://example.test\")"},
            {"name": " \t=cmd|' /C calc'!A0"},
        ],
        ["name"],
    )

    with path.open(newline="", encoding="utf-8") as handle:
        rows = list(csv.DictReader(handle))

    assert [row["name"] for row in rows] == [
        "'=cmd|' /C calc'!A0",
        "'+SUM(1,1)",
        "'-1+2",
        "'@HYPERLINK(\"https://example.test\")",
        "' \t=cmd|' /C calc'!A0",
    ]
