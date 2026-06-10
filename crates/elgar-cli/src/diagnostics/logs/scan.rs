//! System-log file scanning and legacy turn-summary extraction.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::LogsDiagnosticError;

pub(super) fn system_log_files_newest_first(
    log_dir: &Path,
) -> Result<Vec<PathBuf>, LogsDiagnosticError> {
    if !log_dir.exists() {
        return Err(LogsDiagnosticError::LogDirectoryMissing(
            log_dir.to_path_buf(),
        ));
    }

    let mut entries = fs::read_dir(log_dir)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl"))
                .then_some(path)
        })
        .filter_map(|path| {
            let modified = fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|(modified, _path)| *modified);
    entries.reverse();
    let paths = entries
        .into_iter()
        .map(|(_modified, path)| path)
        .collect::<Vec<_>>();

    if paths.is_empty() {
        Err(LogsDiagnosticError::NoSystemLogs(log_dir.to_path_buf()))
    } else {
        Ok(paths)
    }
}

pub(super) fn latest_turn_perf_summary(path: &Path) -> Result<Value, LogsDiagnosticError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?;
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("summary").and_then(Value::as_str) == Some("turn_perf_summary"))
        .last()
        .ok_or_else(|| LogsDiagnosticError::NoTurnPerfSummary(path.to_path_buf()))
}
