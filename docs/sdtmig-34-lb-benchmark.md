# Synthetic LB CLI benchmark

On 2026-11-18, both engines found exactly the 100 deliberately introduced
issues in a **1,000,000-row synthetic LB XPT v5** (18 columns, 141,003,280
bytes). Both used SDTMIG 3.4 and the same five published rule YAMLs. Neither
used any other domain or external dictionary.

## Results

Each engine started six times. All five rules ran on every start, with zero
skips and no failed invocations. Each report matched the expected 20 findings
per rule, including the finding at record 1,000,000. The comparison checks
rule ID, dataset, 1-based record number, the complete variable list,
synthetic subject ID, and sequence. It ignores variable order but retains
duplicate findings and does not apply scorer normalization. SQL also
reported loading 1,000,000 rows on each run.

| Rule | Populated field that made the planted record fail |
|---|---|
| `CORE-000289` | `LBORNRHI` on a categorical result |
| `CORE-000290` | `LBORNRLO` on a categorical result |
| `CORE-000298` | `LBSTNRLO` on a categorical result |
| `CORE-000299` | `LBSTNRHI` on a categorical result |
| `CORE-002098` | `LBSTNRC` on a numeric result |

| Engine | First million-row run | Repeat median (n=5) | Repeat range |
|---|---:|---:|---:|
| Verisian SQL CORE v1.0.0, temporary PostgreSQL | 8.63 s | 8.47 s | 8.32-9.45 s |
| `core-rs` release | 2.07 s | 2.02 s | 2.01-2.31 s |

The timer starts before launching the process and stops after exit. It
includes startup, XPT reads, SQL database setup and ingestion, rule
execution, JSON output, and cleanup. Data generation, hash checks, and
report comparison run outside the timer. The SQL CLI's own validation
timer started after ingestion and reported 1.5-2.7 s; that is not the wall
time above.

The first pair ran SQL then Rust, after a separate 2,000-row correctness
check. The five repeat pairs alternated engine order and ran sequentially.
Every sample used a fresh process, and SQL started a fresh temporary
PostgreSQL instance with `-mem`. The OS cache was not cleared. The input and
temporary database were on the same external SSD on a 10-logical-CPU
macOS arm64 host with other activity (one-minute load 12.97-17.77 at sample
starts). The numbers compare these CLI configurations only. They do not
estimate persistent-database performance, clinical conformance, or results
for other rule sets or study sizes.

## Reproduce the comparison

Run this pilot on macOS; the driver uses the macOS `/usr/bin/time -l`
invocation. Use Python 3.12, R with `haven`, and a writable SSD with space
for the source extraction, XPT, and temporary database. R 4.6.1 and
haven 2.5.5 generated the original fixture. From the repository checkout,
set `SSD_ROOT` to a short, writable path on the SSD.

```sh
REPO=$(git rev-parse --show-toplevel)
BENCH=$(mktemp -d "$SSD_ROOT/lb-bench.XXXXXX")
DB_TMP=$(mktemp -d "$SSD_ROOT/lbpg.XXXXXX")
SQL_COMMIT=4a734105540a83f1df80d061df5049bd4b6bda4d
RULE_COMMIT=94963e01df448f05bdbbc43a482661d4f30f7dac
SQL_TGZ="$BENCH/sql-core.tar.gz"
RULE_TGZ="$BENCH/core-contributor.tar.gz"
SQL_SOURCE="$BENCH/cdisc-rules-engine-$SQL_COMMIT"
SQL_VENV="$BENCH/venv"

curl -fL "https://codeload.github.com/verisianHQ/cdisc-rules-engine/tar.gz/$SQL_COMMIT" -o "$SQL_TGZ"
curl -fL "https://codeload.github.com/cdisc-org/core-contributor/tar.gz/$RULE_COMMIT" -o "$RULE_TGZ"
```

Before extraction, run `shasum -a 256 "$SQL_TGZ" "$RULE_TGZ"`. Stop if
either digest differs.

| Archive | Expected SHA-256 |
|---|---|
| Verisian SQL engine | `09772a78ed91d62d6f029c6abfaeee48fd68b72b6773673dbe8fd3d44fcaf949` |
| core-contributor | `bac26a001ab45f60b5f6b9f7d2f41a71506f10ac6ec41cfc2df1fee381dece30` |

```sh
tar -xzf "$SQL_TGZ" -C "$BENCH"
python3.12 -m venv "$SQL_VENV"
"$SQL_VENV/bin/python" -m pip install -r "$SQL_SOURCE/requirements.txt"
Rscript --vanilla "$REPO/scripts/sdtmig_34_lb_benchmark/generate.R" 1000000 "$BENCH/lb-million"
```

Set `BUNDLE` to a [release-verified](release-reproducibility.md) directory
with `bin/core-rs` and `release-manifest.json`. The measured binary was the
macOS arm64 release build at `6ce2e19c4dbce6cef50dcadf9bfade0e83ae6c93`
(SHA-256 `1271adaf03b0755115a4a2785ceca809b0a4dc762352bf3410343543cbf0721b`).
The binary and XPT are not in Git. If you do not have that binary, build
`core-cli` with `cargo build --locked --release -p core-cli` from a clean
checkout of that commit. Record and verify its manifest before running.
Use `--allow-rebuild` only for a verified rebuild with different bytes;
its timing is a new measurement. The original XPT hash was
`de5e789a70394c5df08f1c9364ec5cfc2532b69d1e769a32716518d748b547fa`.
Regenerating the same logical rows can change the file hash because the
XPT header records a timestamp.

```sh
"$SQL_VENV/bin/python" -B -E -s "$REPO/scripts/sdtmig_34_lb_benchmark/bench.py" \
  --bench "$BENCH" --bundle "$BUNDLE" --rule-archive "$RULE_TGZ" \
  --db-tmp "$DB_TMP" --input "$BENCH/lb-million" \
  --rows 1000000 --repeats 5 --label replay
```

Expect 12 observations, each with five rules executed, 100 findings,
zero skips, and `expected_match: true`. The driver fails rather than count
a skipped rule, missing result, or mismatch as a timing sample.
The repository replay matched all 12 observations on the original input;
its timings are not included in the table above.
`replay/inputs.json` records input and rule hashes; `measurements.json`
records individual runs; `summary.json` separates initial and repeated
times. Use a fresh label to repeat. A long temporary DB path can prevent
PostgreSQL from opening its Unix socket, so keep `DB_TMP` short.
Keep the generated XPT and detailed reports outside version control.

## Resident PostgreSQL follow-up

On 2026-11-18, a second run used the same million-row XPT, five rule YAMLs,
official SQL CORE v1.0.0 source, and Rust release binary. Both SQL
configurations used PostgreSQL 16.2 from the same `pgserver` package. The
resident server used a new private cluster with a Unix socket only. The
runner verified that the server did not restart between requests and
stopped it after the run. The temporary SQL CLI used `-mem` and a fresh
cluster for each request.

All 18 invocations (six per configuration) ran all five rules without
skips. Each report matched the same 100 expected findings, including
the last row, using the same structural comparison described above.

| Configuration | First CLI run | Repeat median (n=5) | Repeat range |
|---|---:|---:|---:|
| Official SQL, resident PostgreSQL already ready | 8.38 s | 7.53 s | 7.49-7.61 s |
| Official SQL, new temporary PostgreSQL each time | 8.55 s | 8.48 s | 8.38-8.66 s |
| `core-rs` release, no database | 2.16 s | 2.05 s | 2.03-2.11 s |

The CLI timer includes launch, XPT loading, rule execution, JSON writing,
and exit. The temporary result includes cluster initialization and
startup. The resident result excludes the one-time server setup.

| SQL phase | Resident first | Resident repeat median | Temporary first | Temporary repeat median |
|---|---:|---:|---:|---:|
| Initialize cluster | 0.634 s | Not repeated | 0.592 s | 0.584 s |
| Start server until ready | 0.132 s | Not repeated | 0.145 s | 0.146 s |
| Load XPT into PostgreSQL, including indexes | 2.28 s | 2.29 s | 2.35 s | 2.28 s |
| Execute five rules, excluding cleanup and JSON writing | 1.49 s | 1.49 s | 1.49 s | 1.49 s |

The separate resident setup took 0.767 s in total. Adding that to the
first resident CLI run gives about 9.14 s. Temporary initialization
and startup together took 0.737 s initially and a median of 0.731 s on
repeats. With the resident server already ready, the repeat CLI median
was 0.95 s shorter than with a temporary server. Both SQL modes still
loaded the XPT on every run; loading and rule-execution times were nearly
the same. Rust was not broken down into individual phases.

The phase probe times selected functions without editing the official
SQL files. The `db_pool` phase in the output includes temporary cluster
initialization and startup; do not sum it with those phases. Phase times
do not account for all interpreter work, metadata setup, or cleanup.
The first group ran resident SQL, temporary SQL, then Rust. The six
sequential groups covered all six execution orders. OS caches were not
flushed. The macOS arm64 host had 10 logical CPUs and other activity
(one-minute load 6.76-19.00 at sample starts). These results describe
this workload and these CLI configurations, not a tuned PostgreSQL
deployment or a service that keeps the dataset in memory.

### Replay the three-way comparison

Complete the preceding replay first. Keep the XPT and expected findings
under `BENCH/lb-million` and the YAMLs and hashes under the reference
run's directory. Use the same bundle, SQL source tree, and Python
environment. Set `--reference-label` to the label of the two-way run.

```sh
RESIDENT_OUT=$(mktemp -d "$BENCH/resident-replay.XXXXXX")
PG_ROOT=$(mktemp -d "$SSD_ROOT/lbg.XXXXXX")
"$SQL_VENV/bin/python" -B -E -s "$REPO/scripts/sdtmig_34_lb_benchmark/compare_postgres.py" \
  --bench "$BENCH" --reference-label replay --bundle "$BUNDLE" \
  --out "$RESIDENT_OUT" --pg-root "$PG_ROOT"
```

Use empty, private output and database directories. Keep the DB root short
for the macOS Unix socket limit. Do not connect this benchmark to an
existing database: the SQL validator drops its run tables. The driver
starts and stops a separate local server, checks the database mode, and
verifies the finding identities on every run before accepting its time.
Expect 18 valid observations in `measurements.json`, first-run and repeat
aggregates in `summary.json`, and the one-time setup in
`resident-startup.json`. The repository replay matched all 18 observations
on the original input; its timings are not included above. Keep generated
files, XPTs, and raw logs outside Git.
