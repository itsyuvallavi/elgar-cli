use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

const TRACE_ENV: &str = "ELGAR_TRACE";
const TRACE_DIR_ENV: &str = "ELGAR_TRACE_DIR";
const TRACE_RELATIVE_DIR: &str = ".elgar/traces";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalTraceEvent {
    pub trace_id: String,
    pub session_id: String,
    pub turn_index: u64,
    pub kind: String,
    pub timestamp_unix_ms: u128,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

pub(crate) fn new_trace_id(session_id: &str, turn_index: u64) -> String {
    format!("{}-turn-{turn_index}", safe_trace_component(session_id))
}

pub(crate) fn append_trace_event(
    project_root: &Path,
    session_id: &str,
    trace_id: &str,
    turn_index: u64,
    kind: impl Into<String>,
    metadata: Value,
) -> std::io::Result<()> {
    if trace_disabled() {
        return Ok(());
    }

    let event = LocalTraceEvent {
        trace_id: trace_id.to_string(),
        session_id: session_id.to_string(),
        turn_index,
        kind: kind.into(),
        timestamp_unix_ms: unix_time_millis(),
        metadata,
    };
    let path = trace_file_path(project_root, session_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(&event)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub(crate) fn trace_file_path(project_root: &Path, session_id: &str) -> PathBuf {
    if let Some(dir) = env::var_os(TRACE_DIR_ENV).filter(|value| !value.is_empty()) {
        return PathBuf::from(dir).join(format!("{}.jsonl", safe_trace_component(session_id)));
    }
    project_root
        .join(TRACE_RELATIVE_DIR)
        .join(format!("{}.jsonl", safe_trace_component(session_id)))
}

fn trace_disabled() -> bool {
    match env::var(TRACE_ENV)
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

fn safe_trace_component(value: &str) -> String {
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
