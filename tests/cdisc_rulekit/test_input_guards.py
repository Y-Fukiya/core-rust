import zipfile

import pytest

from cdisc_rulekit.cli import main
from cdisc_rulekit.errors import CliUsageError
from cdisc_rulekit.inputs import locate_open_rules_repo, resolve_open_rules_input
from cdisc_rulekit.load_open_rules import discover_rule_yml, load_open_rules


def _legacy(root, *, scope="Published", count=1, content="Core:\n  Id: CORE-000001\n"):
    for index in range(count):
        rule = root / scope / f"CORE-{index:06d}" / "rule.yml"
        rule.parent.mkdir(parents=True, exist_ok=True)
        rule.write_text(content)
    return root


@pytest.mark.parametrize("mixed", [False, True])
def test_contributor_and_mixed_layouts_cannot_bypass_guards(tmp_path, mixed):
    root = tmp_path / "repo"
    (root / "sdtm_rules").mkdir(parents=True)
    if mixed:
        _legacy(root)
    for call in (locate_open_rules_repo, discover_rule_yml, load_open_rules):
        with pytest.raises(CliUsageError, match="unsupported.*sdtm_rules"):
            call(root)


@pytest.mark.parametrize("other_layout", ["legacy", "v1"])
def test_ambiguous_wrapper_is_not_resolved_by_rule_count(tmp_path, other_layout):
    first = _legacy(tmp_path / "large", count=3)
    second = tmp_path / "small"
    if other_layout == "legacy":
        _legacy(second)
    else:
        (second / "sdtm_rules").mkdir(parents=True)
    with pytest.raises(CliUsageError, match="ambiguous") as error:
        locate_open_rules_repo(tmp_path)
    assert str(first) in str(error.value)
    assert str(second) in str(error.value)
    assert locate_open_rules_repo(first) == first


def test_single_legacy_child_and_exact_root_work(tmp_path):
    root = _legacy(tmp_path / "wrapper" / "repo")
    (root.parent / "unrelated").mkdir()
    assert locate_open_rules_repo(root.parent) == root
    assert locate_open_rules_repo(root) == root
    assert len(load_open_rules(root)[0]) == 1


def test_single_contributor_child_is_explicitly_unsupported(tmp_path):
    (tmp_path / "repo" / "sdtm_rules").mkdir(parents=True)
    with pytest.raises(CliUsageError, match="unsupported.*sdtm_rules"):
        locate_open_rules_repo(tmp_path)


@pytest.mark.parametrize("kind", ["missing", "file", "unknown", "empty-published", "rule-directory"])
def test_missing_or_empty_default_scope_fails_before_outputs(tmp_path, p21_rules_path, capsys, kind):
    root = tmp_path / "input"
    if kind == "file":
        root.write_text("not a repository")
    elif kind == "unknown":
        root.mkdir()
    elif kind == "empty-published":
        (root / "Published").mkdir(parents=True)
    elif kind == "rule-directory":
        (root / "Published" / "CORE-000001" / "rule.yml").mkdir(parents=True)
    with pytest.raises(CliUsageError):
        load_open_rules(root)
    out = tmp_path / "output"
    assert main([
        "build-readonly", "--p21-rules", str(p21_rules_path),
        "--open-rules-repo", str(root), "--out", str(out),
    ]) == 1
    assert not out.exists()
    assert "complete" not in capsys.readouterr().out


@pytest.mark.parametrize("mixed", [False, True])
def test_contributor_build_repro_is_rejected_before_outputs(tmp_path, p21_rules_path, capsys, mixed):
    root = tmp_path / "input"
    (root / "sdtm_rules").mkdir(parents=True)
    if mixed:
        _legacy(root)
    out = tmp_path / "output"
    assert main([
        "build-readonly", "--p21-rules", str(p21_rules_path),
        "--open-rules-repo", str(root), "--out", str(out),
    ]) == 1
    captured = capsys.readouterr()
    assert "unsupported Open Rules format" in captured.err
    assert "0 CORE rules" not in captured.out
    assert not out.exists()


def test_unpublished_requires_explicit_scope(tmp_path):
    root = _legacy(tmp_path / "repo", scope="Unpublished")
    assert locate_open_rules_repo(root) == root
    with pytest.raises(CliUsageError, match="no rule.yml"):
        load_open_rules(root)
    assert len(load_open_rules(root, include_unpublished=True)[0]) == 1


@pytest.mark.parametrize("content", ["Check: [", "- not\n- a mapping\n", "", "{}", "null", "false"])
def test_all_malformed_rules_fail_but_partial_valid_load_preserves_warnings(tmp_path, content):
    root = _legacy(tmp_path / "repo", content=content)
    with pytest.raises(CliUsageError, match="no usable Open Rules"):
        load_open_rules(root)
    good = root / "Published" / "CORE-GOOD" / "rule.yml"
    good.parent.mkdir()
    good.write_text("Core:\n  Id: CORE-GOOD\n")
    rules, _, warnings = load_open_rules(root)
    assert [rule.core_rule_id for rule in rules] == ["CORE-GOOD"]
    assert len(warnings) == 1
    assert "rule.yml" in warnings[0]


@pytest.mark.parametrize("extra", [None, "legacy", "v1"])
def test_zip_wrapper_uses_same_package_guards(tmp_path, extra):
    archive = tmp_path / "rules.zip"
    with zipfile.ZipFile(archive, "w") as handle:
        handle.writestr("repo/Published/CORE-000001/rule.yml", "Core:\n  Id: CORE-000001\n")
        if extra == "legacy":
            handle.writestr("other/Published/CORE-000002/rule.yml", "Core:\n  Id: CORE-000002\n")
        elif extra == "v1":
            handle.writestr("other/sdtm_rules/CORE-000001/arbitrary.yml", "Core:\n  Id: CORE-000001\n")
    if extra:
        with pytest.raises(CliUsageError, match="ambiguous"):
            resolve_open_rules_input(archive, tmp_path / "work")
    else:
        root = resolve_open_rules_input(archive, tmp_path / "work")
        assert len(load_open_rules(root)[0]) == 1
