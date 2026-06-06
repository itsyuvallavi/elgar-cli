//! Public entry point for system JSONL logs.
//!
//! System logs record runtime flow, timings, and diagnostics under
//! `.elgar/log/system`.

mod event;
mod redact;
mod writer;

use std::path::{Path, PathBuf};

pub use event::{LogEvent, LogInput, LogPhase};
use redact::redact_metadata;

pub fn append_log_event(
    project_root: &Path,
    session_id: &str,
    input: LogInput,
) -> std::io::Result<()> {
    writer::append_log_event(project_root, session_id, input, redact_metadata)
}

pub fn log_path(project_root: &Path, session_id: &str) -> PathBuf {
    writer::log_file_path(project_root, session_id)
}

pub fn log_directory(project_root: &Path) -> PathBuf {
    writer::log_dir_path(project_root)
}
