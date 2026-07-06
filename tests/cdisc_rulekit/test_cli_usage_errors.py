import cdisc_rulekit.cli as cli


def test_generate_negative_limit_is_formatted_as_usage_error(capsys):
    result = cli.main(
        [
            "generate",
            "--p21-catalog",
            "missing.csv",
            "--conversion-status",
            "missing-status.csv",
            "--operator-inventory",
            "missing-operators.csv",
            "--out",
            "generated",
            "--limit",
            "-1",
        ]
    )

    captured = capsys.readouterr()
    assert result == 1
    assert captured.err == "error: limit must be zero or greater\n"


def test_export_rules_invalid_target_subdir_is_formatted_as_usage_error(tmp_path, capsys):
    generated = tmp_path / "generated"
    (generated / "P21_SD0001").mkdir(parents=True)
    repo = tmp_path / "open-rules"
    repo.mkdir()

    result = cli.main(
        [
            "export-rules",
            "--generated-rules",
            str(generated),
            "--open-rules-repo",
            str(repo),
            "--target-subdir",
            "../escape",
        ]
    )

    captured = capsys.readouterr()
    assert result == 1
    assert captured.err == "error: target_subdir must be a relative path inside open_rules_repo\n"
