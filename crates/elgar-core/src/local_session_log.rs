use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const SESSION_LOG_ENV: &str = "ELGAR_SESSION_LOG";
const SESSION_LOG_DIR_ENV: &str = "ELGAR_SESSION_LOG_DIR";
const SESSION_LOG_RELATIVE_DIR: &str = ".elgar/sessions";

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
    let path = session_log_file_path(project_root, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(&event).map_err(std::io::Error::other)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub(crate) fn session_log_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    if let Some(dir) = env::var_os(SESSION_LOG_DIR_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(dir).join(format!("{}.jsonl", safe_session_component(session_id)));
    }
    session_log_dir_path(project_root).join(format!("{}.jsonl", safe_session_component(session_id)))
}

pub fn session_log_path(project_root: &Path, session_id: &str) -> PathBuf {
    session_log_file_path(project_root, session_id)
}

pub(crate) fn session_log_dir_path(project_root: &Path) -> PathBuf {
    if let Some(dir) = env::var_os(SESSION_LOG_DIR_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(dir);
    }
    project_root.join(SESSION_LOG_RELATIVE_DIR)
}

pub fn session_log_directory(project_root: &Path) -> PathBuf {
    session_log_dir_path(project_root)
}

fn session_log_disabled() -> bool {
    match env::var(SESSION_LOG_ENV)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
    {
        Some(value) if matches!(value.as_str(), "1" | "on" | "true" | "local") => false,
        Some(value) if matches!(value.as_str(), "0" | "off" | "false" | "disabled" | "none") => {
            true
        }
        Some(_) => false,
        None if cfg!(test) => true,
        None => false,
    }
}

fn unix_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn safe_session_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .take(96)
        .collect::<String>();

    if component.is_empty() {
        "session".to_string()
    } else {
        component
    }
}
