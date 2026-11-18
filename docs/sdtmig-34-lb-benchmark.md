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
