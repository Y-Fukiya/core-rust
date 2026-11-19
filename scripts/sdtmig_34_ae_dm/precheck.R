sdtmig_34_ae_dm_precheck <- function(ae_xpt, dm_xpt, bundle_dir, qualification_file,
                                   lock_file, rules_dir, project_root, timeout_ms = 30000L) {
  result <- list(status = "not_evaluated", checked_rules = NA_integer_,
                 finding_count = NA_integer_, log_path = NULL)
  reason <- "qualification"
  sha <- function(path) digest::digest(path, algo = "sha256", file = TRUE)
  read_json <- function(path) {
    jsonlite::read_json(normalizePath(path, mustWork = TRUE), simplifyVector = FALSE)
  }
  quiet <- function(condition) {
    restart <- findRestart(if (inherits(condition, "message")) "muffleMessage" else "muffleWarning")
    if (!is.null(restart)) invokeRestart(restart)
  }
  tryCatch(withCallingHandlers({
    runtime <- tempfile("core-rs-precheck-")
    stopifnot(dir.create(runtime, mode = "0700"))
    on.exit(unlink(runtime, recursive = TRUE), add = TRUE)
    stage <- function(from, to = file.path(runtime, basename(from))) {
      stopifnot(all(file.copy(from, to)))
      to
    }
    lock_copy <- stage(lock_file, file.path(runtime, "pilot-lock.json"))
    lock <- read_json(lock_copy)
    proof <- read_json(stage(qualification_file, file.path(runtime, "qualification.json")))
    ids <- vapply(lock$rules, `[[`, character(1), "id")
    domains <- vapply(lock$rules, `[[`, character(1), "domain")
    stopifnot(
      identical(lock$schema_version, 1L), identical(proof$schema_version, 1L),
      identical(lock$standard, "sdtmig"), identical(lock$standard_version, "3.4"),
      identical(proof$status, "qualified"), identical(sha(lock_copy), proof$pilot_lock_sha256),
      length(ids) == 5L, !anyDuplicated(ids), all(grepl("^CORE-[0-9]{6}$", ids)),
      identical(sort(domains), c("AE", "AE", "DM", "DM", "DM")),
      identical(vapply(proof$comparisons, `[[`, character(1), "case"), c("normal", ids)),
      identical(vapply(proof$comparisons, `[[`, integer(1), "executed"), rep(5L, 6L)),
      identical(vapply(proof$comparisons, `[[`, integer(1), "findings"), c(0L, rep(1L, 5L)))
    )
    manifest_file <- stage(file.path(bundle_dir, "release-manifest.json"))
    manifest <- read_json(manifest_file)
    binary <- stage(file.path(bundle_dir, "bin", "core-rs"))
    stopifnot(
      identical(sha(manifest_file), proof$release_manifest_sha256),
      identical(manifest$git$commit, proof$source_commit),
      isTRUE(manifest$git$available), identical(manifest$git$dirty, FALSE),
      identical(manifest$target_triple, proof$target_triple),
      identical(manifest$artifacts, list(list(path = "bin/core-rs", sha256 = proof$binary_sha256))),
      identical(sha(binary), proof$binary_sha256), Sys.chmod(binary, "0500")
    )

    reason <- "rule_pin"
    rule_files <- stage(file.path(rules_dir, paste0(ids, ".yml")))
    stopifnot(identical(vapply(rule_files, sha, character(1), USE.NAMES = FALSE),
                        vapply(lock$rules, `[[`, character(1), "sha256")),
              all(Sys.chmod(rule_files, "0400")))

    reason <- "input"
    root <- normalizePath(project_root, mustWork = TRUE)
    paths <- normalizePath(c(AE = ae_xpt, DM = dm_xpt), mustWork = TRUE)
    names(paths) <- c("AE", "DM")
    stopifnot(identical(unname(basename(paths)), c("ae.xpt", "dm.xpt")),
              all(startsWith(paths, paste0(root, "/"))),
              is.numeric(timeout_ms), length(timeout_ms) == 1L,
              is.finite(timeout_ms), timeout_ms >= 1)
    input_hashes <- vapply(paths, sha, character(1))

    reason <- "log_directory"
    log_base <- root
    for (component in c("output", "core-rs-precheck")) {
      log_base <- file.path(log_base, component)
      stopifnot(is.na(Sys.readlink(log_base)) || !nzchar(Sys.readlink(log_base)))
      if (!dir.exists(log_base)) {
        dir.create(log_base, mode = "0700", showWarnings = FALSE)
      }
      stopifnot(identical(normalizePath(log_base, mustWork = TRUE), log_base))
    }
    out <- tempfile("run-", tmpdir = log_base)
    stopifnot(dir.create(out, mode = "0700"))
    result$log_path <- file.path("output", "core-rs-precheck", basename(out))

    reason <- "input_changed"
    snapshot_dir <- file.path(out, "inputs")
    stopifnot(dir.create(snapshot_dir, mode = "0700"))
    on.exit(unlink(snapshot_dir, recursive = TRUE), add = TRUE)
    snapshots <- setNames(file.path(snapshot_dir, basename(paths)), names(paths))
    stopifnot(all(file.copy(paths, snapshots)),
              identical(input_hashes, vapply(snapshots, sha, character(1))))

    reason <- "input"
    data <- lapply(names(snapshots), function(domain) {
      dataset <- haven::read_xpt(snapshots[[domain]])
      rules <- lock$rules[domains == domain]
      keys <- c("DOMAIN", "USUBJID", if (domain == "AE") "AESEQ")
      required <- c(keys, unlist(lapply(rules, `[[`, "variables"), use.names = FALSE))
      stopifnot(nrow(dataset) > 0L, all(required %in% names(dataset)),
                all(dataset$DOMAIN == domain))
      dataset[keys]
    })
    names(data) <- names(snapshots)

    reason <- "engine"
    run <- processx::run(
      binary, c("validate", "-s", "sdtmig", "-v", "3.4", "--dataset-path", snapshots,
                "--local-rules", rule_files, "--output-format", "json", "-o", out),
      wd = out,
      stdout = file.path(out, "stdout.log"), stderr = file.path(out, "stderr.log"),
      timeout = timeout_ms / 1000, error_on_status = FALSE, cleanup_tree = TRUE
    )
    if (isTRUE(run$timeout)) reason <- "timeout"
    stopifnot(identical(run$status, 0L), !isTRUE(run$timeout))
    reason <- "input_changed"
    stopifnot(identical(input_hashes, vapply(paths, sha, character(1))))

    reason <- "report"
    report <- read_json(file.path(out, "report.json"))
    meta <- report$metadata
    stopifnot(identical(meta$schema_version, "1.0"), identical(meta$engine, "core-rs"),
              identical(meta$standard, "sdtmig"), identical(meta$standard_version, "3.4"),
              identical(meta$dataset_count, 2L), identical(meta$rule_count, 5L))
    results <- report$results
    actual_ids <- vapply(results, `[[`, character(1), "rule_id")
    stopifnot(identical(sort(actual_ids), sort(ids)))
    counts <- vapply(seq_along(ids), function(i) {
      r <- results[[match(ids[[i]], actual_ids)]]
      domain <- domains[[i]]
      count <- length(r$errors)
      wanted_status <- if (count) "failed" else "passed"
      stopifnot(is.list(r$errors), identical(r$dataset, domain), identical(r$domain, domain),
                identical(r$execution_status, wanted_status), is.null(r$skipped_reason),
                identical(r$error_count, count))
      for (issue in r$errors) {
        rows <- data[[domain]]
        row <- issue$row
        stopifnot(is.integer(row), length(row) == 1L, row >= 1L, row <= nrow(rows),
                  identical(issue$rule_id, ids[[i]]), identical(issue$dataset, domain),
                  identical(issue$domain, domain),
                  identical(sort(unlist(issue$variables)), sort(unlist(lock$rules[[i]]$variables))),
                  identical(issue$usubjid, rows$USUBJID[[row]]))
        if (domain == "AE") {
          stopifnot(!is.na(rows$AESEQ[[row]]), identical(issue$seq, as.character(rows$AESEQ[[row]])))
        } else {
          stopifnot(is.null(issue$seq))
        }
      }
      count
    }, integer(1))
    total <- sum(counts)
    summary <- report$summary
    stopifnot(identical(summary$total_results, 5L), identical(summary$skipped, 0L),
              identical(summary$passed, sum(counts == 0L)),
              identical(summary$failed, sum(counts > 0L)), identical(summary$error_count, total))

    result$status <- if (total) "findings" else "clean"
    result$checked_rules <- length(ids)
    result$finding_count <- total
    result
  }, warning = quiet, message = quiet), error = function(e) {
    result$reason <- reason
    result
  })
}
