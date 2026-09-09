use std::fs::{self, File, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::{ReportError, Result};

// Reserve the whole bundle, including formats disabled for this run. A stale
// CSV or log must never be mistaken for a newly written JSON report's sibling.
pub(super) struct ReportDirectoryGuard {
    path: PathBuf,
    file: Option<File>,
}

impl ReportDirectoryGuard {
    pub(super) fn reserve(output_dir: &Path) -> Result<Self> {
        fs::create_dir_all(output_dir).map_err(|source| ReportError::CreateDir {
            path: output_dir.to_path_buf(),
            source,
        })?;
        let path = output_dir.join(".core-rs-report.lock");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| {
                if source.kind() == ErrorKind::AlreadyExists {
                    ReportError::OutputReserved { path: path.clone() }
                } else {
                    ReportError::CreateFile {
                        path: path.clone(),
                        source,
                    }
                }
            })?;
        let guard = Self {
            path,
            file: Some(file),
        };
        for name in ["report.json", "report.csv", "validation.log"] {
            let path = output_dir.join(name);
            match fs::symlink_metadata(&path) {
                Ok(_) => return Err(ReportError::ExistingReport { path }),
                Err(source) if source.kind() == ErrorKind::NotFound => {}
                Err(source) => return Err(ReportError::InspectReport { path, source }),
            }
        }
        Ok(guard)
    }
}

impl Drop for ReportDirectoryGuard {
    fn drop(&mut self) {
        // Close before removal for Windows. A failed removal conservatively
        // leaves the directory reserved; a later run must use a different one.
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn concurrent_reservation_is_rejected_and_owner_releases_lock() {
        let dir = tempdir().unwrap();
        let guard = ReportDirectoryGuard::reserve(dir.path()).unwrap();
        assert!(matches!(
            ReportDirectoryGuard::reserve(dir.path()),
            Err(ReportError::OutputReserved { .. })
        ));
        assert!(dir.path().join(".core-rs-report.lock").exists());
        drop(guard);
        assert!(!dir.path().join(".core-rs-report.lock").exists());
        assert!(ReportDirectoryGuard::reserve(dir.path()).is_ok());
    }

    #[test]
    fn rejected_existing_report_releases_own_lock() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("report.csv"), "old report").unwrap();
        assert!(matches!(
            ReportDirectoryGuard::reserve(dir.path()),
            Err(ReportError::ExistingReport { .. })
        ));
        assert!(!dir.path().join(".core-rs-report.lock").exists());
    }
}
