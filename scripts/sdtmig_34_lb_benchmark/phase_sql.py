"""Time selected operations in the pinned SQL engine without editing its files."""

import functools
import json
import os
import runpy
import sys
import time
from pathlib import Path


def timed(original, records, name):
    @functools.wraps(original)
    def wrapper(*args, **kwargs):
        start = time.perf_counter()
        try:
            return original(*args, **kwargs)
        finally:
            records.setdefault(name, []).append(time.perf_counter() - start)
    return wrapper


def observe_pg(records):
    from pgserver.postgres_server import PostgresServer

    for method, name in (("ensure_pgdata_inited", "initdb"),
                         ("ensure_postgres_running", "db_start_ready")):
        setattr(PostgresServer, method, timed(getattr(PostgresServer, method), records, name))


def main():
    records = {}
    core = Path(sys.argv[1]).resolve()
    sys.path.insert(0, str(core.parent))
    sys.argv = sys.argv[1:]
    if "-mem" in sys.argv:
        observe_pg(records)
    from cdisc_rules_engine.data_service.database import DatabasePostgres
    from cdisc_rules_engine.data_service.loading.load_datasets import SqlDatasetLoader
    from scripts import run_sql_validation as validator

    init_pool = DatabasePostgres._init_pool

    def verified_pool(self):
        records.setdefault("pool_config", []).append(type(self.config).__name__)
        return init_pool(self)

    DatabasePostgres._init_pool = timed(verified_pool, records, "db_pool")
    SqlDatasetLoader.load_datasets = staticmethod(timed(
        SqlDatasetLoader.load_datasets, records, "load_xpt_to_db"
    ))
    validator.sql_validate_single_rule = timed(
        validator.sql_validate_single_rule, records, "validate_rule"
    )
    try:
        runpy.run_path(core, run_name="__main__")
    finally:
        Path(os.environ["LB_PHASES"]).write_text(json.dumps(records, indent=2) + "\n")


if __name__ == "__main__":
    main()
