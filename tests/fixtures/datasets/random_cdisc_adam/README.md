# random.cdisc.data ADaM fixture

This fixture is a small deterministic subset generated from cached
`random.cdisc.data` ADaM datasets:

- CRAN: https://CRAN.R-project.org/package=random.cdisc.data
- Repository: https://github.com/insightsengineering/random.cdisc.data/
- Source license: Apache-2.0
- Upstream description: random SDTM and ADaM datasets for clinical reporting

The fixture is intentionally small and is used only as a second-layer
regression fixture after the official Open Rules oracle harness. It keeps
representative ADaM domains (`ADSL`, `ADAE`, and `ADLB`) so the validation
engine exercises subject-level, adverse-event, lab, and analysis-flag shaped
data without vendoring the full upstream package.
