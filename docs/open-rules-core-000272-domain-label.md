# CORE-000272: standard domain labels

CORE-000272 compares `--CAT` with the domain label, not the domain
abbreviation. The old adapter chose the abbreviation for every SENDIG
request and added `DOMAIN` to the report variables. That made candidate
negative/03 pass even though LB Record 2 has `LBCAT=Laboratory`.

The [CORE v1.0.0 Apple Silicon release](https://github.com/cdisc-org/cdisc-rules-engine/releases/tag/v1.0.0)
ran the candidate snapshot `1881cb9a1f9a38b6cafc785eca5169b240981f66` with
the bundled cache and each fixture's own `.env`. All seven official
replays reproduced their stored CSV. Negative/03 reported LB Record 2
with `LBCAT` and `$domain_name`, both with value `Laboratory`. The
fixture's own label is `Laboratory Test Results`; the official operation
looks up the label in the selected standard metadata instead.

An official control used the same four input categories under three
profiles, keeping the fixture label unchanged:

| Selected profile | Official LB label | Only matching Record |
| --- | --- | ---: |
| SENDIG 3.1 | Laboratory | 2 |
| SDTMIG 3.2 | Labs | 3 |
| SDTMIG 3.4 | Laboratory Test Results | 4 |

The Rust adapter uses the verified LB and CM entries by product,
version and domain from `standards_details.pkl` in the shipped cache
(SHA-256 `e77831cfcc68c1645f087c17e3f0a85d09e82754325b0f0678f58c53f2140d87`).
Other profiles and domains use the existing dataset-label fallback.
The authored output variables are not widened, and other rules do not
get this binding.

For the label lookup, the record's DOMAIN takes precedence over a split
member name. A SENDIG 3.1 official control renamed the LB file to
LBHEM while keeping DOMAIN=LB and still reported Record 2.

The corrected Rust reports match all seven candidate replays on dataset,
record, variables, USUBJID and sequence. The full candidate run changed
only negative/03: supported mismatches fell from 27 to 26, with 17
deferred mismatches unchanged.

## Accepted snapshot boundary

The accepted snapshot `7f7fae49376b3d023563ebb6c36a3b392d6e649f` has
`LBCAT=LB` at Record 2 in negative/03 and negative/04. Its stored CSVs
expect a finding in both cases, with `DOMAIN` as an output variable.
Fresh CORE v1.0.0 reports no findings in either case. The other six
accepted cases reproduced their stored CSVs.

The [curated CI run for `2aa30da`](https://github.com/Y-Fukiya/core-rust/actions/runs/44044824492)
failed the upstream subset gate. A local run with the same inputs matched
276 of 278 cases: only negative/03 and negative/04 differed from the
stored CSVs (two stored issue rows versus zero Rust rows in each).
A separate v1.0.0 replay confirmed `SUCCESS` and CSV headers with no
issue rows in both cases.

The [reviewed curated manifest](../tests/open_rules/curated-reviewed/CORE-000272/manifest.json)
pins the accepted rule tree and those two replay CSVs. The curated
preparation script replaces only those CSVs in the copied subset.
With these replacements the subset matched all 278 cases and passed
the current baseline comparison. The accepted pin, original oracles,
scorer and baseline are unchanged.

Local evidence is under `target/core-272-domain-label-audit/`. The
`official/`, `controls/`, and `split-control/` subdirectories contain
input copies, official reports and process logs. Final scoreboards are
under `final/`.
