//! Writes local session event JSONL records.
//!
//! Session logs record the private event/data truth for a conversation.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::logs::common::{
    append_jsonl, env_var_matches, non_empty_env_path, safe_component, unix_time_millis,
};

const SESSION_LOG_ENV: &str = "ELGAR_SESSION_LOG";
const SESSION_LOG_DIR_ENV: &str = "ELGAR_SESSION_LOG_DIR";
const SESSION_LOG_RELATIVE_DIR: &str = ".elgar/log/sessions";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalSessionLogEvent {
    pub session_id: String,
    pub turn_index: u64,
    pub kind: String,
    pub timestamp_unix_ms: u128,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

pub(crate) fn append_session_event(
    project_root: &Path,
    session_id: &str,
    turn_index: u64,
    kind: impl Into<String>,
    metadata: Value,
) -> std::io::Result<()> {
    if session_log_disabled() {
        return Ok(());
    }

    let event = LocalSessionLogEvent {
        session_id: session_id.to_string(),
        turn_index,
        kind: kind.into(),
        timestamp_unix_ms: unix_time_millis(),
        metadata,
    };
    append_jsonl(&session_log_file_path(project_root, session_id), &event)
}

pub(crate) fn session_log_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    if let Some(dir) = non_empty_env_path(SESSION_LOG_DIR_ENV) {
        return dir.join(format!("{}.jsonl", safe_component(session_id, "session")));
    }
    session_log_dir_path(project_root)
        .join(format!("{}.jsonl", safe_component(session_id, "session")))
}

pub(crate) fn session_log_dir_path(project_root: &Path) -> PathBuf {
    if let Some(dir) = non_empty_env_path(SESSION_LOG_DIR_ENV) {
        return dir;
    }
    project_root.join(SESSION_LOG_RELATIVE_DIR)
}

fn session_log_disabled() -> bool {
    if env_var_matches(SESSION_LOG_ENV, &["1", "on", "true", "local"]) {
        return false;
    }
    if env_var_matches(SESSION_LOG_ENV, &["0", "off", "false", "disabled", "none"]) {
        return true;
    }
    cfg!(test) && std::env::var(SESSION_LOG_ENV).is_err()
}
