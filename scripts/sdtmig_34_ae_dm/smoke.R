args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) == 5L)
pilot_dir <- normalizePath(args[[1L]], mustWork = TRUE)
bundle_dir <- normalizePath(args[[2L]], mustWork = TRUE)
receipt <- normalizePath(args[[3L]], mustWork = TRUE)
normal_dir <- normalizePath(args[[4L]], mustWork = TRUE)
mutants_dir <- normalizePath(args[[5L]], mustWork = TRUE)
source(file.path(pilot_dir, "precheck.R"))

local({
  project <- tempfile("ae dm precheck ")
  stopifnot(dir.create(project, mode = "0700"))
  on.exit(unlink(project, recursive = TRUE))
  lock_file <- file.path(pilot_dir, "pilot-lock.json")
  lock <- jsonlite::read_json(lock_file)
  rules_dir <- file.path(normal_dir, "rules")
  ae <- file.path(project, "ae.xpt")
  dm <- file.path(project, "dm.xpt")
  sha <- function(path) digest::digest(path, algo = "sha256", file = TRUE)
  check <- function(expected, count, reason = NULL, ...) {
    before <- vapply(c(ae, dm), sha, character(1))
    messages <- capture.output(
      summary <- sdtmig_34_ae_dm_precheck(
        ae, dm, bundle_dir, receipt, lock_file, rules_dir, project, ...
      ),
      type = "message"
    )
    stopifnot(length(messages) == 0L, identical(summary$status, expected),
              identical(summary$reason, reason))
    if (is.na(count)) {
      stopifnot(is.na(summary$finding_count), is.na(summary$checked_rules))
    } else {
      stopifnot(summary$finding_count == count, summary$checked_rules == 5L)
      stopifnot(file.exists(file.path(project, summary$log_path, "report.json")))
      stopifnot(grepl("^output/core-rs-precheck/[^/]+$", summary$log_path))
      stopifnot(!bitwAnd(as.integer(file.info(file.path(project, summary$log_path))$mode), 63L))
    }
    stopifnot(identical(vapply(c(ae, dm), sha, character(1)), before))
    stopifnot(!grepl("USUBJID|/private/tmp/|/Volumes/", jsonlite::toJSON(summary)))
    invisible(summary)
  }
  write_pair <- function(input) {
    for (dataset in c("ae", "dm")) {
      file <- paste0(dataset, ".xpt")
      rows <- haven::read_xpt(file.path(input, file))
      haven::write_xpt(rows, file.path(project, file), version = 5,
                      name = toupper(dataset), adjust_tz = FALSE)
    }
  }

  write_pair(normal_dir)
  clean <- check("clean", 0)
  for (rule in lock$rules) {
    write_pair(file.path(mutants_dir, rule$control$folder))
    result <- check("findings", 1)
    stopifnot(!identical(clean$log_path, result$log_path))
    report <- jsonlite::read_json(file.path(project, result$log_path, "report.json"))
    issue <- do.call(c, lapply(report$results, `[[`, "errors"))[[1L]]
    stopifnot(identical(issue$rule_id, rule$id), identical(issue$dataset, rule$domain),
              identical(issue$row, rule$control$row),
              identical(sort(unlist(issue$variables)), sort(unlist(rule$variables))))
  }
  write_pair(normal_dir)
  rows <- haven::read_xpt(ae)
  rows$AESHOSP <- NULL
  haven::write_xpt(rows, ae, version = 5, name = "AE", adjust_tz = FALSE)
  check("not_evaluated", NA_integer_, "input")

  write_pair(normal_dir)
  original_rules <- rules_dir
  rules_dir <- file.path(project, "rules")
  stopifnot(dir.create(rules_dir))
  stopifnot(all(file.copy(list.files(original_rules, full.names = TRUE), rules_dir)))
  cat("\n# changed\n", file = file.path(rules_dir, "CORE-000087.yml"), append = TRUE)
  check("not_evaluated", NA_integer_, "rule_pin")

  rules_dir <- original_rules
  tampered <- file.path(project, "qualification.json")
  proof <- jsonlite::read_json(receipt)
  proof$binary_sha256 <- strrep("0", 64)
  jsonlite::write_json(proof, tampered, auto_unbox = TRUE)
  original_receipt <- receipt
  receipt <- tampered
  check("not_evaluated", NA_integer_, "qualification")

  receipt <- original_receipt
  original_bundle <- bundle_dir
  bundle_dir <- file.path(project, "bundle")
  stopifnot(dir.create(file.path(bundle_dir, "bin"), recursive = TRUE))
  stopifnot(file.copy(file.path(original_bundle, "release-manifest.json"), bundle_dir))
  copied <- file.path(bundle_dir, "bin", "core-rs")
  stopifnot(file.copy(file.path(original_bundle, "bin", "core-rs"), copied))
  cat("changed", file = copied, append = TRUE)
  check("not_evaluated", NA_integer_, "qualification")

  rules_dir <- file.path(project, "rules")
  edited_rule <- file.path(rules_dir, "CORE-000087.yml")
  stopifnot(file.copy(file.path(original_rules, basename(edited_rule)), edited_rule, overwrite = TRUE),
            file.copy(file.path(original_bundle, "bin", "core-rs"), copied, overwrite = TRUE))
  local({
    trace("run", tracer = function() {
      writeLines("replaced executable", copied)
      writeLines("replaced rule", edited_rule)
    }, where = asNamespace("processx"), print = FALSE)
    on.exit(untrace("run", where = asNamespace("processx")))
    list_runtime <- function() list.files(tempdir(), pattern = "^core-rs-precheck-")
    before <- list_runtime()
    check("clean", 0)
    stopifnot(identical(before, list_runtime()), identical(readLines(copied), "replaced executable"),
              identical(readLines(edited_rule), "replaced rule"))
  })

  bundle_dir <- original_bundle
  rules_dir <- original_rules
  check("not_evaluated", NA_integer_, "timeout", timeout_ms = 1L)
  stopifnot(check("clean", 0)$status == "clean")

  swapped <- file.path(mutants_dir, "087", "ae.xpt")
  local({
    trace("run", tracer = function() {
      stopifnot(file.copy(swapped, ae, overwrite = TRUE))
    }, where = asNamespace("processx"), print = FALSE)
    on.exit(untrace("run", where = asNamespace("processx")))
    changed <- sdtmig_34_ae_dm_precheck(
      ae, dm, bundle_dir, receipt, lock_file, rules_dir, project
    )
    stopifnot(identical(changed$status, "not_evaluated"),
              identical(changed$reason, "input_changed"))
    log_dir <- file.path(project, changed$log_path)
    stopifnot(!dir.exists(file.path(log_dir, "inputs")))
    report <- jsonlite::read_json(file.path(log_dir, "report.json"))
    stopifnot(identical(report$summary$error_count, 0L))
  })
})
cat("R post-write precheck: five finding locations, six unavailable paths, and runtime snapshots passed\n")
