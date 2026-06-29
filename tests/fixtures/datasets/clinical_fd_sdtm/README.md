# clinical_fd SDTM fixture

This fixture is a small deterministic subset generated from the `clinical_fd`
fake clinical data package:

- Repository: https://github.com/sas2r/clinical_fd
- Source license: MIT
- Upstream description: fake SDTM/ADaM data for generating TFL

The fixture is intentionally small and is used only for regression tests. It
keeps representative SDTM domains (`DM`, `AE`, `CM`, `SUPPAE`, `RELREC`, `VS`,
and `LB`) so the validation engine exercises multi-domain, supplemental
qualifier, relationship, and larger-row-shape data without vendoring the full
upstream package.
