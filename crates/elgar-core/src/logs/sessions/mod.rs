//! Public entry point for session JSONL logs.
//!
//! Session logs record conversation event truth under `.elgar/log/sessions`.

mod writer;

use std::path::{Path, PathBuf};

pub use writer::LocalSessionLogEvent;

pub(crate) fn append_session_event(
    project_root: &Path,
    session_id: &str,
    turn_index: u64,
    kind: impl Into<String>,
    metadata: serde_json::Value,
) -> std::io::Result<()> {
    writer::append_session_event(project_root, session_id, turn_index, kind, metadata)
}

pub fn session_log_path(project_root: &Path, session_id: &str) -> PathBuf {
    writer::session_log_file_path(project_root, session_id)
}

pub fn session_log_directory(project_root: &Path) -> PathBuf {
    writer::session_log_dir_path(project_root)
}
