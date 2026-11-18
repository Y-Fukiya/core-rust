# Try the AE and DM precheck on R exports

This pilot runs five fixed SDTMIG 3.4 checks on AE and DM XPT v5 files.
It is an optional developer check, not a submission validator. The bundle
includes a qualified `core-rs` for Apple silicon macOS, the exact five YAMLs,
and two public R sample objects. You do not need the repository, Rust, SQL,
or PostgreSQL to run the example.

1. Use native arm64 R on an Apple silicon Mac. The clean-library trial used
   macOS and R 4.6.1. Obtain the archive and its SHA-256 from a trusted source.
   Verify the hash before extracting or running code. The release manifest
   and qualification receipt are records, not signatures.

   ```sh
   shasum -a 256 core-rs-ae-dm-pilot-macos-arm64.tar.gz
   tar -xzf core-rs-ae-dm-pilot-macos-arm64.tar.gz
   ```

2. In a private local folder, use a fresh R library and install the four
   packages. The CRAN download requires network access.

   ```sh
   mkdir -p r-pilot-library
   export R_LIBS_USER="$PWD/r-pilot-library"
   Rscript --vanilla -e 'install.packages(c("haven", "jsonlite", "digest", "processx"), repos="https://cran.r-project.org", type="binary")'
   ```

3. Run the example with a new project directory. Start in the folder that
   contains the extracted bundle. The example creates `pilot-project`; use
   a different name on each retry.

   ```sh
   BUNDLE="$PWD/core-rs-ae-dm-pilot-macos-arm64"
   Rscript --vanilla "$BUNDLE/example.R" "$BUNDLE" "$PWD/pilot-project"
   ```

   Expect a clean normal run, one matching finding in each of the five
   abnormal cases, `not_evaluated (input)` for a missing column, and a clean
   recheck after restoring the original value. The example does not print
   subject IDs, cell values, or full paths.

## Check your own exports

Call the helper after both successful XPT writes. Use lowercase `ae.xpt`
and `dm.xpt` below your project root. Resolve the bundle and project paths
before calling this example.

```r
source(file.path(bundle, "precheck.R"))
summary <- sdtmig_34_ae_dm_precheck(
  ae_xpt = file.path(project, "ae.xpt"),
  dm_xpt = file.path(project, "dm.xpt"),
  bundle_dir = bundle,
  qualification_file = file.path(bundle, "release-qualification.json"),
  lock_file = file.path(bundle, "pilot-lock.json"),
  rules_dir = file.path(bundle, "rules"),
  project_root = project
)
```

All five rules must execute for `clean` or `findings`. A skipped rule or a
missing column returns `not_evaluated`, not zero findings. The check leaves
the input XPTs unchanged. Whether you continue is up to your pipeline.
If present, `summary$log_path` is relative to the project root and points
to private logs. Keep generated XPTs and `output/core-rs-precheck/` out
of Git and shared storage. Do not paste detailed logs or reports into a
model or an online service.

If `reason` is `qualification` or `rule_pin`, verify and re-extract the
trusted archive rather than replacing individual files. For `input`,
check the input filenames, folders, and required columns. For `engine`,
check the platform and whether R can run the verified binary from its
temporary directory. Do not bypass system security restrictions.

## Included sources

- `bin/core-rs`: the macOS arm64 candidate identified by the release manifest.
- `rules/`: [core-contributor](https://github.com/cdisc-org/core-contributor/tree/94963e01df448f05bdbbc43a482661d4f30f7dac),
  five unmodified rule files.
- `sample-data/`: [pharmaversesdtm](https://github.com/pharmaverse/pharmaversesdtm/tree/7ec53ea88dfe1c168f9efbef59233c4426813b89/data),
  two unmodified public test data objects. They are not certified SDTMIG
  3.4 data. New XPTs can have different header timestamps than the
  qualification control XPTs.

License notices are in `licenses/`. The archive contains no generated XPTs,
raw reports, or execution logs.
