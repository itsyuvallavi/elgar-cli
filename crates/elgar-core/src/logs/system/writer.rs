//! Writes log events to local JSONL files.
//!
//! This file owns paths and append behavior only. It does not decide what the
//! events mean.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::logs::common::{
    append_jsonl, env_var_matches, non_empty_env_path, safe_component, unix_time_millis,
};

use super::{LogEvent, LogInput};

const LOG_ENV: &str = "ELGAR_LOG";
const LOG_DIR_ENV: &str = "ELGAR_LOG_DIR";
const LOG_RELATIVE_DIR: &str = ".elgar/log/system";

pub(super) fn append_log_event(
    project_root: &Path,
    session_id: &str,
    input: LogInput,
    redact_metadata: impl FnOnce(Value) -> Value,
) -> std::io::Result<()> {
    if log_disabled() {
        return Ok(());
    }

    let timestamp = unix_time_millis();
    let event = LogEvent {
        session_id: session_id.to_string(),
        turn_id: input.turn_id,
        event_id: format!(
            "turn-{}-{}-{}",
            input.turn_id,
            timestamp,
            safe_component(input.summary, "event")
        ),
        parent_event_id: None,
        timestamp_unix_ms: timestamp,
        phase: input.phase,
        file: input.file.to_string(),
        function: input.function.to_string(),
        summary: input.summary.to_string(),
        duration_ms: input.duration_ms,
        metadata: redact_metadata(input.metadata),
    };

    append_jsonl(&log_file_path(project_root, session_id), &event)
}

pub(super) fn log_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    log_dir_path(project_root).join(format!("{}.jsonl", safe_component(session_id, "event")))
}

pub(super) fn log_dir_path(project_root: &Path) -> PathBuf {
    if let Some(dir) = non_empty_env_path(LOG_DIR_ENV) {
        return dir;
    }
    project_root.join(LOG_RELATIVE_DIR)
}

fn log_disabled() -> bool {
    env_var_matches(LOG_ENV, &["0", "off", "false", "disabled", "none"])
        || (cfg!(test) && std::env::var(LOG_ENV).is_err())
}
