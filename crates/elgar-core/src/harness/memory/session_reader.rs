//! Session JSONL reader for durable harness memory.
//!
//! The reader is intentionally read-only. Missing session logs return an empty
//! event list so memory can be optional.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
};

use crate::logs::sessions::{session_log_path, LocalSessionLogEvent};

#[derive(Debug)]
pub enum SessionMemoryReadError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Json {
        path: PathBuf,
        line: usize,
        source: serde_json::Error,
    },
}

impl fmt::Display for SessionMemoryReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "failed to read {}: {source}", path.display())
            }
            Self::Json { path, line, source } => {
                write!(
                    formatter,
                    "failed to parse {} line {line}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for SessionMemoryReadError {}

pub fn read_session_memory_events(
    project_root: &Path,
    session_id: &str,
) -> Result<Vec<LocalSessionLogEvent>, SessionMemoryReadError> {
    let path = session_log_path(project_root, session_id);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&path).map_err(|source| SessionMemoryReadError::Io {
        path: path.clone(),
        source,
    })?;

    content
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str::<LocalSessionLogEvent>(line).map_err(|source| {
                SessionMemoryReadError::Json {
                    path: path.clone(),
                    line: index + 1,
                    source,
                }
            })
        })
        .collect()
}
