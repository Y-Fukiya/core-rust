use std::path::{Path, PathBuf};

use polars::prelude::DataFrame;

use crate::{DataError, Result};

pub(crate) fn column_names(frame: &DataFrame) -> Vec<String> {
    frame
        .get_column_names()
        .into_iter()
        .map(|name| name.as_str().to_owned())
        .collect()
}

pub(crate) fn file_name(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| DataError::InvalidDatasetPackage(format!("missing file name: {path:?}")))
}

pub(crate) fn file_stem(path: &Path) -> Result<String> {
    path.file_stem()
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .ok_or_else(|| DataError::InvalidDatasetPackage(format!("missing file stem: {path:?}")))
}

pub(crate) fn file_stem_str(filename: &str) -> &str {
    Path::new(filename)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(filename)
}

pub(crate) fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

pub(crate) fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
}
