# AE/DM precheck pilot: input and normal control

On 2026-11-18 Verisian SQL v1.0.0 and the corrected Rust reader executed all
five SDTMIG 3.4 rules on the same AE and DM XPT v5 files. Both reported zero
issues on the unmodified inputs and the expected record and variables in all
five one-cell abnormal controls. These are pilot results, not release
qualification or Hook usability evidence. No upstream SHA or baseline changed.

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

## Before enabling a Hook

1. Repeat the normal and abnormal controls on the release binary before
   distributing it. Missing results, skips, load errors and capped reports
   cannot establish agreement.
2. Call the local precheck from R only after output files are closed. Pin the
   platform binary and its SHA-256 against a trusted release manifest, verify
   with [release-verify](release-reproducibility.md), and reject a mismatch.
   Keep full diagnostics in access-controlled project-local files, outside
   Git and model context. Return only status, allowlisted rule IDs/variable
   names, counts and a neutral project-relative log path; never echo stderr,
   data values or subject identifiers. A failed run must say *not evaluated*.

This pilot can test reproducibility and, once tried by an R programmer, Hook
usability. It does not measure the value of retaining the Rust engine.
Use a separate realistic synthetic LB workload for that decision: include SQL
startup and data load, report cold and warm runs separately, measure binary
provisioning effort, and use independently labelled defects when comparing
detection. MCP, SAS and ADaM integration are outside this pilot.
