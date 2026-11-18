# AE/DM precheck pilot

On 2026-11-18 Verisian SQL v1.0.0 and the corrected Rust reader executed all
five SDTMIG 3.4 rules on the same AE and DM XPT v5 files. Both reported zero
issues on the unmodified inputs and the expected record and variables in all
five one-cell abnormal controls. These are pilot results, not a submission
validation or an R workflow usability study. No upstream SHA or baseline changed.

## Inputs and reproduction

- SQL: [Verisian v1.0.0](https://github.com/verisianHQ/cdisc-rules-engine/tree/4a734105540a83f1df80d061df5049bd4b6bda4d),
  Python 3.12.2, dependencies from the release's `requirements.txt`. Run
  from the engine source root with `-mem` (a disposable database); do not
  point the engine at an existing study database.
- Rules: [core-contributor](https://github.com/cdisc-org/core-contributor/tree/94963e01df448f05bdbbc43a482661d4f30f7dac/sdtm_rules).
  Copy just the single rule YAML from each of the five ID directories below
  into an empty `rules/` directory. Both engines must receive those same bytes.
  No contributor Excel fixture or saved result is used.
- Data: [pharmaversesdtm](https://github.com/pharmaverse/pharmaversesdtm/tree/7ec53ea88dfe1c168f9efbef59233c4426813b89/data),
  `ae.rda` (SHA-256 `dae58611f970a2bed9071999da3c390548c66d58a1e5430472a935aadc887c17`)
  and `dm.rda` (`36364acc1d1e16fc72d6b28d04a9677df2375f543a46abe75d609de9835176fb`).
  The package is a source of public examples, not a certified SDTMIG 3.4 study.

In a fresh pilot directory, load the pinned public R objects and write the
files **once** for both engines (haven 2.5.5):

```r
for (n in c("ae", "dm")) {
  e <- new.env()
  load(paste0(n, ".rda"), envir = e)
  haven::write_xpt(e[[n]], paste0(n, ".xpt"), version = 5,
                  name = toupper(n), adjust_tz = FALSE)
}
```

Set `P` to the absolute pilot directory. From the SQL source root:

```sh
python core.py validate -mem -s sdtmig -v 3-4 \
  -dp "$P/ae.xpt" -dp "$P/dm.xpt" -lr "$P/rules" \
  -of JSON -o "$P/sql-normal" -p disabled

"$CORE_RS" validate -s sdtmig -v 3.4 \
  --dataset-path "$P/ae.xpt" "$P/dm.xpt" --local-rules "$P/rules" \
  --output-format json -o "$P/rust-normal" --strict
```

Verify `Dataset_Details` shows AE (1,191 records) and DM (306 records).
The XPTs have 35 and 28 variables respectively. SHA-256 used in this run:
AE `d7ce20e18e33097e32188063ddc5fbce16d9180b7ea07c3c7882aea1a2384862`,
DM `481baf47c2747eb996650c7e83ab81ef0b9e99e833b8d404345f999e9e431724`.
Record fresh hashes when exporting again: XPT headers can carry timestamps.
Equal input bytes control the export step, not the engines' internal casts.

## Normal controls

| Rule | Domain | Condition that raises an issue | SQL status | Rust status |
|---|---|---|---|---|
| CORE-000087 | AE | Nonblank AESER outside the literal Y/N pair | SUCCESS, 0 | passed, 0 |
| CORE-000127 | AE | Nonblank AESHOSP outside the literal Y/N pair | SUCCESS, 0 | passed, 0 |
| CORE-000119 | DM | ARM populated while ARMCD is blank | SUCCESS, 0 | passed, 0 |
| CORE-000185 | DM | ACTARMCD longer than 20 characters | SUCCESS, 0 | passed, 0 |
| CORE-000189 | DM | AGE present while AGEU is blank | SUCCESS, 0 | passed, 0 |

The SQL report identified CORE 1.0.0, SDTMIG V3.4, no Define-XML or
dictionary and no selected CT version. Both issue arrays were empty. As a
negative control for **execution status**, running cached CORE-000001 alone on
the same files returned exit 0 and empty issue arrays, but its rule status was
`SKIPPED` as it did not apply to AE/DM. This is not a successful rule check.

Before the fix, the Rust diagnostic returned exit 1, `XPT NAMESTR header is missing variable count`,
with no rule report. This was reproduced with a fresh `cargo build -p core-cli
--locked --offline` at `03fed24da9778f532141212e19719b3bb9305264`.
The diagnostic binary SHA-256 was
`9326e66a054866785af808f365d080a4a03edf855785d71be9426f998db79084`;
it is a local dev build, not an approved release binary.
The NAMESTR count field in these inputs (zero-based bytes 54..58, end
exclusive) is `0035` or `0028`. The old parser read the trailing 30 bytes as
one integer, including the reserved zeros. The corrected parser reads the
four-character field; the local debug binary used for this comparison had
SHA-256 `d79e9a3c496bac5bcf514843edf9bd97679c458e4c7475b506df334b1adc9f1e`. See the
[SAS v5 record layout, section 6](https://www.sas.com/content/dam/sasdam/documents/20260302/record-layout-of-a-sas-version-5-or-6-data-set-in-sas-transport-xport-format.pdf).
Do not rewrite XPT headers or substitute CSV on just one side to get a match.

## One-cell abnormal controls

After verifying both normal reports, start each case from the original XPTs.
Use `haven::read_xpt()` on the domain to change, assign the one cell below,
and use the same v5 export settings. Copy the other domain byte for byte.
Re-read the exported file and compare every column in record order to the
original with that one edit applied. A blank in this table is the empty
character string; the 21-character value is the ASCII string
`ABCDEFGHIJKLMNOPQRSTU`.

| Rule | Domain, 1-based record | Cell change | Reported variables in both engines |
|---|---|---|---|
| CORE-000087 | AE, 2 | AESER: N to X | AETERM, AESER |
| CORE-000127 | AE, 7 | AESHOSP: N to X | AESHOSP |
| CORE-000119 | DM, 12 | ARMCD: Pbo to blank | ARMCD, ARM |
| CORE-000185 | DM, 19 | ACTARMCD: Scrnfail to 21 characters | ACTARMCD |
| CORE-000189 | DM, 23 | AGEU: YEARS to blank | AGE, AGEU |

Run all five rules in each case with fresh output paths. For the abnormal
controls the Rust CLI was run without `--strict`; success was determined from
the JSON, not the process exit code. Each report had exactly one finding
and the other four rules ran clean. Compare rule ID, domain, record and
the complete list of variable names (ignoring order, retaining multiplicity)
against the table, not just between engines. For all five findings, USUBJID
matched the source row. For AE the sequence matched 2 and 4 respectively
(JSON number in SQL, string in Rust); for DM it was absent (empty string in
SQL, null in Rust). No scorer relaxation or CSV substitution was used.

## Qualify the local release candidate

The [pilot lock](../scripts/sdtmig_34_ae_dm/pilot-lock.json) fixes the five YAML
digests, SQL source, original AE and DM XPT bytes, edited XPT bytes, and
expected finding locations. Use the exact archived files for qualification;
a new haven export may have a different XPT header timestamp. The lock is
specific to this pilot. It does not replace the adopted Open Rules lock or
its baselines. Do not commit XPTs or reports from a user project.

Build `core-rs` and `xtask` with
`cargo build --locked --release -p core-cli -p xtask` from a clean checkout.
Place the binary in `$BUNDLE/bin/core-rs` and use
[`xtask release-manifest`](release-reproducibility.md) to record exactly that
artifact relative to the bundle root. Set `SOURCE_DATE_EPOCH` to your
reviewed source timestamp before generating the manifest. For a local pilot
there is no CI run URL; this is not the full publishing gate.

Use the SQL engine's Python environment, which provides `pyreadstat`, and
run the qualification driver with a fresh output directory:

```sh
"$SQL_VENV/bin/python" -B scripts/sdtmig_34_ae_dm/qualify.py \
  --lock scripts/sdtmig_34_ae_dm/pilot-lock.json \
  --bundle "$BUNDLE" --source-root "$SOURCE" --xtask "$XTASK" \
  --sql-archive "$SQL_ARCHIVE" \
  --normal "$NORMAL" --mutants "$MUTANTS" --out "$QUAL_OUT"
```

`NORMAL` contains `ae.xpt`, `dm.xpt` and `rules/`; `MUTANTS` contains
an AE/DM pair in each `control.folder` from the lock. The driver checks
and freshly extracts the pinned SQL archive, then runs both engines against
the same digest-checked input copies. It runs `release-verify` on a private
copy of the bundle. Allow space for the temporary extraction under
`QUAL_OUT`. The driver removes the copies and writes `qualification.json`
only after all six comparisons pass. Keep the receipt and bundle in a
trusted local directory; these hashes are not a signature. Qualify again
when the binary, platform or rule lock changes.

The local `aarch64-apple-darwin` release candidate at
`6ce2e19c4dbce6cef50dcadf9bfade0e83ae6c93` passed all six comparisons on
2026-11-18. Its SHA-256 was
`1271adaf03b0755115a4a2785ceca809b0a4dc762352bf3410343543cbf0721b`.
The [qualification receipt](../scripts/sdtmig_34_ae_dm/release-qualification.json)
records five clean executions per engine on the normal control and one
expected record/variable finding on each abnormal control.

## Check after writing XPTs in R

The optional check uses `haven`, `jsonlite`, `digest` and `processx`.
Call it after both successful `haven::write_xpt()` calls have returned.
Use lowercase `ae.xpt` and `dm.xpt` under `project_root`. The runtime checks
the freshly written study data; it does not require the public sample's
content or row counts. Keep `output/core-rs-precheck/` out of version
control and shared storage. This repository already ignores `output/`;
add the exclusion to any other project before using the check there.

```r
source(file.path(pilot_dir, "precheck.R"))
summary <- sdtmig_34_ae_dm_precheck(
  ae_xpt = ae_path, dm_xpt = dm_path,
  bundle_dir = bundle_dir, qualification_file = receipt_path,
  lock_file = file.path(pilot_dir, "pilot-lock.json"),
  rules_dir = rules_dir, project_root = project_root
)
# Continue the export workflow regardless of summary$status.
```

The call waits at most 30 seconds for the validator subprocess by default.
It returns `clean`, `findings` or `not_evaluated`. Skipped, missing or
inconsistent results, load failure, and a changed rule or binary never count
as zero findings. The helper does not change either XPT or fail the calling
pipeline when the check is unavailable. Haven and the validator read the
same private XPT copies. R also copies the executable and five YAML files
to a private temporary directory, checks those copies against the
qualification receipt and lock, and launches only the verified copies.
The helper removes all these copies before returning. Changes to the
original XPT files during the check yield `not_evaluated`.
The safe summary contains counts
and a neutral project-relative log directory. Raw reports, stdout and stderr
stay in its private run directory. Do not pass those files to a model.

Run the smoke check with real R writes and the verified binary:

```sh
Rscript --vanilla scripts/sdtmig_34_ae_dm/smoke.R \
  scripts/sdtmig_34_ae_dm "$BUNDLE" "$QUAL_OUT/qualification.json" \
  "$NORMAL" "$MUTANTS"
```

## Hand off the pilot bundle

Use the [bundle quick start](../scripts/sdtmig_34_ae_dm/README.md) on the
recipient's Apple silicon Mac. The archive includes the qualified binary,
its manifest and receipt, the pilot lock, five YAMLs, R scripts, the two
pinned public R objects, and license notices. It does not include generated
XPTs, private study data, SQL binaries, or logs. The recipient needs R and the four
packages but no source checkout or database.

To assemble it, use the unchanged qualified release directory as
`RELEASE_DIR` and the five pinned YAML files under `RULES_DIR`. Set
`SAMPLE_DIR` to the location of the two public `.rda` objects listed above.
Save the [core-contributor license](https://raw.githubusercontent.com/cdisc-org/core-contributor/94963e01df448f05bdbbc43a482661d4f30f7dac/LICENSE)
as `core-contributor-LICENSE.txt` and the
[pharmaversesdtm license](https://raw.githubusercontent.com/pharmaverse/pharmaversesdtm/7ec53ea88dfe1c168f9efbef59233c4426813b89/LICENSE.md)
as `pharmaversesdtm-LICENSE.md` in `NOTICES_DIR`.

```sh
python3 -B scripts/sdtmig_34_ae_dm/bundle.py \
  --release-dir "$RELEASE_DIR" --rules-dir "$RULES_DIR" \
  --sample-dir "$SAMPLE_DIR" --notices-dir "$NOTICES_DIR" \
  --out core-rs-ae-dm-pilot-macos-arm64.tar.gz
```

The builder checks the staged binary, manifest, rules, samples, and
third-party notices before writing an archive. Send the printed SHA-256
through a trusted channel. This packages an existing qualification; it
does not qualify a new build or alter the adopted Open Rules version.

### Clean-environment replay

The engineering replay started with an empty home directory and R library,
installed CRAN arm64 binaries, and ran the extracted archive from an unrelated
directory. Both the bundle and project paths contained spaces. It used R
4.6.1, haven 2.5.5, jsonlite 2.0.0, digest 0.6.39, and processx 3.9.0.
No source checkout, SQL engine, or database was needed at runtime. The
normal and repaired controls each ran five checks with zero findings.
The five one-cell abnormal controls matched the locked rule, domain,
record, and output variables. An export missing a required column returned
`not_evaluated` with reason `input` and no finding count.

The regression replay also rejected altered qualification inputs and
verified that changing the source executable and a YAML after staging
does not change which bytes run. The sandboxed trial could not complete
subprocess cleanup, so the end-to-end evidence is from the normal macOS
execution environment. This is an engineering replay on one Mac, not
acceptance by a separate human operator. An independent automated operator
also followed the bundled quick start from a new private directory, using
the fresh R library without accessing the repository, and reproduced the
four result lines. For human acceptance, record the archive hash, R version,
and the four result lines on the recipient's Mac. Do not include private
reports in the handoff record.

## R export and repair trial

On 2026-11-18, the R helper at `7d8552e` ran the qualified macOS arm64
binary on exports from the pinned public AE (1,191 rows) and DM (306 rows)
objects. No SQL process was needed during the R workflow.

- The draft deliberately set AE row 7 `AESHOSP` to `X`. All five rules
  ran. Only `CORE-000127` reported a finding, at that row and variable.
- The repair set the value back to `N` in R and wrote a fresh XPT pair.
  All five rules ran with zero findings and zero skips. Five more exports
  and checks in the same R process gave the same result.
- Reading both pairs back showed that only the intended AE cell changed
  and the DM data was unchanged. The precheck left both XPT file hashes
  unchanged in each run.

### Observed wait

| Run (macOS arm64, R 4.6.1) | Write both XPTs | Post-write check |
|---|---:|---:|
| Draft, first invocation in a fresh R process | 155 ms | 301 ms |
| Repair, first invocation in a fresh R process | 154 ms | 296 ms |
| Repair, same R process (five further runs) | median 7 ms (6-7 ms) | median 205 ms (182-215 ms) |

The check timer covered the R helper, including hashes, private copies,
validator startup, and report interpretation. The first write can include
haven package loading. The times exclude R startup, loading the public
objects, installing dependencies, reading the report as an operator, and
editing the mapping. They are local observations on the small public pair,
not a performance comparison or an estimate for larger studies.

### Setup checks

Use the qualified executable with its matching manifest, receipt and pilot
lock, and preserve the exact five YAMLs. A rebuild needs its own
qualification. The four R packages were already installed for this trial;
package installation time was not measured.

| Condition tried | Returned status and reason | What to check |
|---|---|---|
| Missing qualification receipt | `not_evaluated` / `qualification` | Path and match to the bundle and lock |
| Missing YAML directory | `not_evaluated` / `rule_pin` | Location and exact bytes of the five YAMLs |
| Uppercase `AE.xpt` | `not_evaluated` / `input` | Lowercase filenames under the project root |
| Child process restricted by the Codex sandbox | `not_evaluated` / `engine` | Whether the R runtime can launch the validator |

The safe summary contains no row location. Open the private report locally
to decide what to repair. Do not treat `not_evaluated` as a zero count or
share XPTs and detailed logs through Git or a model.

### Replay the public trial

Use a fresh, private, Git-ignored `PROJECT` directory. In R, supply
`PILOT_SCRIPTS` (the repository's `scripts/sdtmig_34_ae_dm` directory),
`NORMAL` (the pinned `ae.rda`, `dm.rda` and `rules/`), `BUNDLE` (the
verified release bundle), and `QUALIFICATION_JSON` (its receipt) as
environment variables. Use the same source and binary pins as above.

```r
project <- normalizePath(Sys.getenv("PROJECT"), mustWork = TRUE)
pilot <- Sys.getenv("PILOT_SCRIPTS")
normal <- Sys.getenv("NORMAL")
source(file.path(pilot, "precheck.R"))
public <- new.env()
load(file.path(normal, "ae.rda"), envir = public)
load(file.path(normal, "dm.rda"), envir = public)
stopifnot(nrow(public$ae) == 1191L, nrow(public$dm) == 306L,
          identical(public$ae$AESHOSP[[7L]], "N"))

export_and_check <- function(stage, hospital_flag) {
  ae <- public$ae
  ae$AESHOSP[[7L]] <- hospital_flag
  folder <- file.path(project, stage)
  stopifnot(dir.create(folder, mode = "0700"))
  ae_file <- file.path(folder, "ae.xpt")
  dm_file <- file.path(folder, "dm.xpt")
  haven::write_xpt(ae, ae_file, version = 5, name = "AE", adjust_tz = FALSE)
  haven::write_xpt(public$dm, dm_file, version = 5, name = "DM", adjust_tz = FALSE)
  sdtmig_34_ae_dm_precheck(
    ae_xpt = ae_file, dm_xpt = dm_file, project_root = project,
    bundle_dir = Sys.getenv("BUNDLE"),
    qualification_file = Sys.getenv("QUALIFICATION_JSON"),
    lock_file = file.path(pilot, "pilot-lock.json"),
    rules_dir = file.path(normal, "rules")
  )
}

draft <- export_and_check("draft", "X")
stopifnot(draft$status == "findings", draft$checked_rules == 5L,
          draft$finding_count == 1L)
report <- jsonlite::read_json(file.path(project, draft$log_path, "report.json"))
issue <- do.call(c, lapply(report$results, `[[`, "errors"))[[1L]]
stopifnot(issue$rule_id == "CORE-000127", issue$dataset == "AE",
          issue$row == 7L, identical(issue$variables, list("AESHOSP")))
print(issue[c("rule_id", "dataset", "row", "variables")])

# Repair the R input and export again. Do not patch the XPT.
repaired <- export_and_check("repaired", public$ae$AESHOSP[[7L]])
stopifnot(repaired$status == "clean", repaired$checked_rules == 5L,
          repaired$finding_count == 0L)
```

The assertions are for this replay only. A real pipeline should continue
and display the advisory status regardless of the precheck outcome.
See the separate [million-row synthetic LB benchmark](sdtmig-34-lb-benchmark.md)
for measured CLI startup and data load costs. MCP, SAS and ADaM are outside
this pilot.
