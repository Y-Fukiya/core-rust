args <- commandArgs(trailingOnly = TRUE)
stopifnot(length(args) == 2L)
n <- as.integer(args[[1]])
out <- args[[2]]
stopifnot(n >= 200L, dir.create(out, mode = "0700"))
i <- seq_len(n)
subject <- (i - 1L) %/% 100L + 1L
test <- (i - 1L) %% 5L + 1L
is_cat <- test == 4L
value <- (i %% 80L) + 4
lb <- data.frame(
  STUDYID = "SYNLB01", DOMAIN = "LB",
  USUBJID = sprintf("SYNLB01-%06d", subject), LBSEQ = (i - 1L) %% 100L + 1L,
  LBTESTCD = c("ALT", "AST", "HGB", "URPROT", "WBC")[test],
  LBTEST = c("Alanine Aminotransferase", "Aspartate Aminotransferase", "Hemoglobin",
             "Urine Protein", "White Blood Cell Count")[test],
  LBSPEC = ifelse(is_cat, "URINE", "BLOOD"),
  LBORRES = ifelse(is_cat, "NEGATIVE", sprintf("%d", value)),
  LBORRESU = c("U/L", "U/L", "g/L", "", "10^9/L")[test],
  LBORNRLO = ifelse(is_cat, "", "3"), LBORNRHI = ifelse(is_cat, "", "70"),
  LBSTRESC = ifelse(is_cat, "NEGATIVE", sprintf("%d", value)),
  LBSTRESN = ifelse(is_cat, NA_real_, value),
  LBSTRESU = c("U/L", "U/L", "g/L", "", "10^9/L")[test],
  LBSTNRLO = ifelse(is_cat, NA_real_, 3), LBSTNRHI = ifelse(is_cat, NA_real_, 70),
  LBSTNRC = ifelse(is_cat, "NEGATIVE", ""),
  LBDTC = sprintf("2026-01-%02d", (i - 1L) %% 28L + 1L)
)

ids <- c("CORE-000289", "CORE-000290", "CORE-000298", "CORE-000299", "CORE-002098")
fields <- c("LBORNRHI", "LBORNRLO", "LBSTNRLO", "LBSTNRHI", "LBSTNRC")
ends <- floor(seq_len(20L) * n / 20L)
expected <- lapply(seq_along(ids), function(k) {
  row <- ends - (length(ids) - k)
  if (k < length(ids)) {
    lb$LBTESTCD[row] <<- "URPROT"
    lb$LBTEST[row] <<- "Urine Protein"
    lb$LBSPEC[row] <<- "URINE"
    lb$LBORRES[row] <<- "TRACE"
    lb$LBORRESU[row] <<- ""
    lb$LBORNRLO[row] <<- ""
    lb$LBORNRHI[row] <<- ""
    lb$LBSTRESC[row] <<- "TRACE"
    lb$LBSTRESN[row] <<- NA_real_
    lb$LBSTRESU[row] <<- ""
    lb$LBSTNRLO[row] <<- NA_real_
    lb$LBSTNRHI[row] <<- NA_real_
    lb$LBSTNRC[row] <<- "NEGATIVE"
  } else {
    lb$LBTESTCD[row] <<- "ALT"
    lb$LBTEST[row] <<- "Alanine Aminotransferase"
    lb$LBSPEC[row] <<- "BLOOD"
    lb$LBORRES[row] <<- "20"
    lb$LBORRESU[row] <<- "U/L"
    lb$LBSTRESC[row] <<- "20"
    lb$LBSTRESN[row] <<- 20
    lb$LBSTRESU[row] <<- "U/L"
  }
  lb[[fields[[k]]]][row] <<- list("70", "3", 3, 70, "NEGATIVE")[[k]]
  data.frame(rule_id = ids[[k]], dataset = "LB", row = row, usubjid = lb$USUBJID[row],
             seq = lb$LBSEQ[row], variables = paste("LBTESTCD", "LBORRES", fields[[k]], sep = "|"))
})
attr(lb, "label") <- "Laboratory Test Results"
xpt <- file.path(out, "lb.xpt")
haven::write_xpt(lb, xpt, version = 5L, name = "LB", adjust_tz = FALSE)
expected <- do.call(rbind, expected)
write.csv(expected, file.path(out, "expected.csv"), row.names = FALSE, quote = TRUE)
saved <- haven::read_xpt(xpt, col_select = c(USUBJID, LBSEQ, LBORRES))
stopifnot(nrow(saved) == n, identical(saved$USUBJID[expected$row], expected$usubjid),
          identical(as.integer(saved$LBSEQ[expected$row]), expected$seq),
          all(saved$LBORRES[expected$row] == rep(c("TRACE", "20"), c(80L, 20L))))
cat(sprintf("LB: %d rows, %d columns, %d bytes, %d planted issues\n",
            n, ncol(lb), file.info(xpt)$size, nrow(expected)))
