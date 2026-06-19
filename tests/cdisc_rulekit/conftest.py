from pathlib import Path

import pytest


@pytest.fixture
def fixture_root() -> Path:
    return Path(__file__).parent / "fixtures"


@pytest.fixture
def p21_rules_path(fixture_root: Path) -> Path:
    return fixture_root / "p21" / "cdisc_rule_definitions_latest_2204.csv"


@pytest.fixture
def p21_domain_map_path(fixture_root: Path) -> Path:
    return fixture_root / "p21" / "cdisc_rule_domain_map.csv"


@pytest.fixture
def open_rules_repo_path(fixture_root: Path) -> Path:
    return fixture_root / "open_rules"
