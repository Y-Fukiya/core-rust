# Public sample only. Use a new, private project outside shared storage.
# Rscript --vanilla <bundle>/example.R <bundle-dir> <new-project-dir>

require_that <- function(ok, message) {
  if (!isTRUE(ok)) stop(errorCondition(message, class = "pilot_example_error"))
}

main <- function(args) {
  require_that(length(args) == 2L, "Supply a bundle directory and a new project directory.")
  packages <- c("haven", "jsonlite", "digest", "processx")
  require_that(all(vapply(packages, requireNamespace, logical(1), quietly = TRUE)),
               "Install haven, jsonlite, digest and processx in your project R library.")
  bundle <- normalizePath(args[[1L]], mustWork = TRUE)
  old_umask <- Sys.umask("0077")
  on.exit(Sys.umask(old_umask), add = TRUE)
  require_that(dir.create(args[[2L]], mode = "0700", showWarnings = FALSE),
               "The project directory must be new and its parent must exist.")
  project <- normalizePath(args[[2L]], mustWork = TRUE)
  lock_file <- file.path(bundle, "pilot-lock.json")
  lock <- jsonlite::read_json(lock_file)
  require_that(length(lock$rules) == 5L, "Expected the five-rule pilot lock.")
  source(file.path(bundle, "precheck.R"), local = TRUE)

  sample <- new.env(parent = emptyenv())
  for (domain in c("AE", "DM")) {
    name <- tolower(domain)
    load(file.path(bundle, "sample-data", paste0(name, ".rda")), envir = sample)
  }
  normal <- list(AE = sample$ae, DM = sample$dm)

  export_and_check <- function(data, folder) {
    stage <- file.path(project, folder)
    require_that(dir.create(stage, mode = "0700"), "Could not create an export folder.")
    ae_xpt <- file.path(stage, "ae.xpt")
    dm_xpt <- file.path(stage, "dm.xpt")
    haven::write_xpt(data$AE, ae_xpt, version = 5, name = "AE", adjust_tz = FALSE)
    haven::write_xpt(data$DM, dm_xpt, version = 5, name = "DM", adjust_tz = FALSE)
    sdtmig_34_ae_dm_precheck(
      ae_xpt = ae_xpt, dm_xpt = dm_xpt, bundle_dir = bundle,
      qualification_file = file.path(bundle, "release-qualification.json"),
      lock_file = lock_file, rules_dir = file.path(bundle, "rules"), project_root = project
    )
  }

  expect_count <- function(result, status, count) {
    identical(result$status, status) && identical(result$checked_rules, 5L) &&
      identical(result$finding_count, count)
  }

  require_that(expect_count(export_and_check(normal, "normal"), "clean", 0L),
               "Normal check did not complete cleanly. Inspect the private logs locally.")

  for (i in seq_along(lock$rules)) {
    rule <- lock$rules[[i]]
    control <- rule$control
    domain <- rule$domain
    column <- control$column
    row <- control$row
    draft <- normal
    # These sample columns are character. Factor or labelled study columns
    # need type-aware assignment; a blank XPT character value may read back as NA.
    original <- draft[[domain]][[column]][[row]]
    require_that(identical(original, control$before), "Sample does not match the mutation lock.")
    draft[[domain]][[column]][[row]] <- control$after
    result <- export_and_check(draft, paste0("abnormal-", i))
    require_that(expect_count(result, "findings", 1L),
                 "An abnormal check did not return exactly one finding. Inspect the private logs locally.")
    report <- jsonlite::read_json(file.path(project, result$log_path, "report.json"))
    issues <- do.call(c, lapply(report$results, `[[`, "errors"))
    require_that(length(issues) == 1L, "Expected exactly one issue in the private report.")
    issue <- issues[[1L]]
    require_that(identical(issue$rule_id, rule$id) && identical(issue$dataset, domain) &&
                   identical(issue$domain, domain) && identical(issue$row, row) &&
                   identical(sort(unlist(issue$variables)), sort(unlist(rule$variables))),
                 "An issue did not match the expected rule, domain, row and variables.")

    # Repair in R, not in the XPT. The final recheck exports this restored pair.
    draft[[domain]][[column]][[row]] <- original
    require_that(identical(draft, normal), "Repair did not restore the original data exactly.")
  }
  repaired <- draft

  missing <- normal
  rule <- lock$rules[[1L]]
  missing[[rule$domain]][[rule$control$column]] <- NULL
  result <- export_and_check(missing, "missing-column")
  require_that(identical(result$status, "not_evaluated") && identical(result$reason, "input") &&
                 identical(result$checked_rules, NA_integer_) && identical(result$finding_count, NA_integer_),
               "A missing required column must return not_evaluated (input), not zero findings.")

  require_that(expect_count(export_and_check(repaired, "repaired"), "clean", 0L),
               "Repaired check did not complete cleanly. Inspect the private logs locally.")
  cat("Normal: 5 rules checked, 0 findings.\n",
      "Abnormal: 5 controls, 5 rules checked per control, 1 matching finding per control.\n",
      "Missing column: not_evaluated (input).\n",
      "Repaired: 5 rules checked, 0 findings.\n", sep = "")
}

# Never echo raw R errors or warnings: they may include paths or report data.
tryCatch(
  suppressMessages(suppressWarnings(main(commandArgs(trailingOnly = TRUE)))),
  error = function(e) {
    if (inherits(e, "pilot_example_error")) cat(conditionMessage(e), "\n", sep = "", file = stderr())
    else cat("Pilot example failed. Check the bundle and inspect any private logs locally.\n", file = stderr())
    quit(save = "no", status = 1L)
  }
)
