from cdisc_rulekit.classify import classify_rules
from cdisc_rulekit.generate_testdata import generate_rule_folder
from cdisc_rulekit.load_p21 import load_p21_rules
from cdisc_rulekit.validate_outputs import validate_generated_rules


def _regex_rule(p21_rules_path, p21_domain_map_path):
    p21_rules, _warnings = load_p21_rules(p21_rules_path, p21_domain_map_path)
    classified = classify_rules(p21_rules, [])
    return next(rule for rule in classified if rule.p21_rule_id == "SD0002")


def test_validate_generated_rules_passes_valid_generated_folder(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
):
    generate_rule_folder(_regex_rule(p21_rules_path, p21_domain_map_path), tmp_path)

    report = validate_generated_rules(tmp_path)

    assert report["summary"]["total"] == 1
    assert report["summary"]["failed"] == 0
    assert report["rules"][0]["valid"] is True


def test_validate_generated_rules_fails_missing_env(
    tmp_path,
    p21_rules_path,
    p21_domain_map_path,
):
    generated = generate_rule_folder(_regex_rule(p21_rules_path, p21_domain_map_path), tmp_path)
    (tmp_path / generated.generated_rule_id / "negative" / "01" / "data" / ".env").unlink()

    report = validate_generated_rules(tmp_path)

    assert report["summary"]["failed"] == 1
    assert any("missing negative/01/data/.env" in error for error in report["rules"][0]["errors"])
