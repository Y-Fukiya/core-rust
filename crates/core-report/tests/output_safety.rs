use std::fs;

use core_report::{write_reports_with_options, ReportError, ReportOptions, ReportOutputFormat};
use tempfile::tempdir;

#[test]
fn any_existing_managed_output_blocks_every_format_without_writing() {
    for existing in ["report.json", "report.csv", "validation.log"] {
        for format in [
            ReportOutputFormat::Both,
            ReportOutputFormat::Json,
            ReportOutputFormat::Csv,
        ] {
            let dir = tempdir().unwrap();
            let path = dir.path().join(existing);
            fs::write(&path, "previous run").unwrap();
            let error = write_reports_with_options(
                dir.path(),
                &[],
                &ReportOptions {
                    output_format: format,
                    ..Default::default()
                },
            )
            .unwrap_err();
            assert!(matches!(error, ReportError::ExistingReport { path: ref p } if p == &path));
            assert_eq!(fs::read_to_string(path).unwrap(), "previous run");
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn unrelated_files_are_preserved_and_do_not_block_reports() {
    let dir = tempdir().unwrap();
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, "human notes").unwrap();
    write_reports_with_options(dir.path(), &[], &ReportOptions::default()).unwrap();
    assert_eq!(fs::read_to_string(notes).unwrap(), "human notes");
    assert!(dir.path().join("report.json").is_file());
    assert!(dir.path().join("report.csv").is_file());
    assert!(!dir.path().join(".core-rs-report.lock").exists());
}

#[test]
fn existing_directory_at_managed_path_blocks_before_partial_json_write() {
    let dir = tempdir().unwrap();
    fs::create_dir(dir.path().join("report.csv")).unwrap();
    assert!(matches!(
        write_reports_with_options(dir.path(), &[], &ReportOptions::default()),
        Err(ReportError::ExistingReport { .. })
    ));
    assert!(!dir.path().join("report.json").exists());
}

#[test]
fn preexisting_reservation_is_preserved_and_blocks_writing() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join(".core-rs-report.lock");
    fs::write(&lock, "other run").unwrap();
    assert!(matches!(
        write_reports_with_options(dir.path(), &[], &ReportOptions::default()),
        Err(ReportError::OutputReserved { .. })
    ));
    assert_eq!(fs::read_to_string(lock).unwrap(), "other run");
    assert!(!dir.path().join("report.json").exists());
}

#[cfg(unix)]
#[test]
fn dangling_symlink_at_managed_path_is_not_followed() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("must-not-be-created");
    std::os::unix::fs::symlink(&target, dir.path().join("report.json")).unwrap();
    assert!(matches!(
        write_reports_with_options(dir.path(), &[], &ReportOptions::default()),
        Err(ReportError::ExistingReport { .. })
    ));
    assert!(!target.exists());
    assert!(!dir.path().join("report.csv").exists());
}
