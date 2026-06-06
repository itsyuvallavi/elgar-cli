//! Shared mechanics for local JSONL logs.
//!
//! Session and system logs own their event shapes. This file only owns common
//! path-safe names, timestamps, environment flags, and append behavior.

use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn env_var_matches(name: &str, values: &[&str]) -> bool {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .is_some_and(|value| values.contains(&value.as_str()))
}

pub(super) fn non_empty_env_path(name: &str) -> Option<std::path::PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

pub(super) fn unix_time_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

pub(super) fn safe_component(value: &str, fallback: &str) -> String {
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
        fallback.to_string()
    } else {
        component
    }
}

pub(super) fn append_jsonl(path: &Path, event: &impl serde::Serialize) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    let line = serde_json::to_string(event).map_err(std::io::Error::other)?;
    writeln!(file, "{line}")?;
    Ok(())
}
