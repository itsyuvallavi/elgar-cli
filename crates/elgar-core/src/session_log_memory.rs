use std::{collections::HashSet, fs, path::PathBuf};

use serde_json::Value;

use crate::{local_session_log, session::Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableVerifiedArtifactFact {
    pub session_id: String,
    pub action_id: String,
    pub turn_index: u64,
    pub operation: String,
    pub path: PathBuf,
    pub source_path: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CappedDurableVerifiedArtifacts {
    pub artifacts: Vec<DurableVerifiedArtifactFact>,
    pub omitted_count: usize,
}

pub fn latest_durable_verified_artifacts(
    session: &Session,
    limit: usize,
) -> CappedDurableVerifiedArtifacts {
    cap_artifacts(
        durable_verified_artifacts(session)
            .into_iter()
            .filter(|artifact| artifact.session_id != session.id)
            .rev(),
        limit,
    )
}

pub fn durable_verified_artifacts(session: &Session) -> Vec<DurableVerifiedArtifactFact> {
    let dir = local_session_log::session_log_dir_path(&session.project_root);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "jsonl")
        })
        .collect::<Vec<_>>();
    files.sort();

    let mut artifacts = Vec::new();
    let mut seen = HashSet::new();
    for file in files {
        let Ok(contents) = fs::read_to_string(file) else {
            continue;
        };
        for line in contents.lines() {
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            for mut fact in durable_facts_from_event(&event) {
                let key = (
                    fact.session_id.clone(),
                    fact.action_id.clone(),
                    fact.operation.clone(),
                    fact.path.clone(),
                );
                if seen.insert(key) {
                    fact.path = normalize_for_session(session, fact.path);
                    fact.source_path = fact
                        .source_path
                        .map(|path| normalize_for_session(session, path));
                    fact.project_root = fact
                        .project_root
                        .map(|path| normalize_for_session(session, path));
                    artifacts.push(fact);
                }
            }
        }
    }
    artifacts
}

fn durable_facts_from_event(event: &Value) -> Vec<DurableVerifiedArtifactFact> {
    match event.get("kind").and_then(Value::as_str) {
        Some("action_applied") => durable_fact_from_action_applied(event)
            .into_iter()
            .collect(),
        Some("memory_selected") => durable_facts_from_memory_selected(event),
        _ => Vec::new(),
    }
}

fn durable_fact_from_action_applied(event: &Value) -> Option<DurableVerifiedArtifactFact> {
    let metadata = event.get("metadata")?;
    let operation = metadata.get("operation")?.as_str()?;
    if operation == "shell_command" {
        return None;
    }
    let path = metadata.get("path")?.as_str()?;
    Some(DurableVerifiedArtifactFact {
        session_id: event.get("session_id")?.as_str()?.to_string(),
        action_id: metadata.get("action_id")?.as_str()?.to_string(),
        turn_index: event.get("turn_index").and_then(Value::as_u64).unwrap_or(0),
        operation: operation.to_string(),
        path: PathBuf::from(path),
        source_path: metadata
            .get("source_path")
            .and_then(Value::as_str)
            .map(PathBuf::from),
        project_root: None,
    })
}

fn durable_facts_from_memory_selected(event: &Value) -> Vec<DurableVerifiedArtifactFact> {
    let Some(selected) = event
        .get("metadata")
        .and_then(|metadata| metadata.get("selected"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    selected
        .iter()
        .filter_map(|fact| {
            if fact.get("kind").and_then(Value::as_str) != Some("verified_artifact") {
                return None;
            }
            Some(DurableVerifiedArtifactFact {
                session_id: event.get("session_id")?.as_str()?.to_string(),
                action_id: fact.get("source_action_id")?.as_str()?.to_string(),
                turn_index: event.get("turn_index").and_then(Value::as_u64).unwrap_or(0),
                operation: "selected_verified_artifact".to_string(),
                path: PathBuf::from(fact.get("path")?.as_str()?),
                source_path: None,
                project_root: fact
                    .get("project_root")
                    .and_then(Value::as_str)
                    .map(PathBuf::from),
            })
        })
        .collect()
}

fn cap_artifacts(
    artifacts: impl IntoIterator<Item = DurableVerifiedArtifactFact>,
    limit: usize,
) -> CappedDurableVerifiedArtifacts {
    let mut artifacts = artifacts.into_iter().collect::<Vec<_>>();
    let omitted_count = artifacts.len().saturating_sub(limit);
    artifacts.truncate(limit);
    CappedDurableVerifiedArtifacts {
        artifacts,
        omitted_count,
    }
}

fn normalize_for_session(session: &Session, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        session.cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_session_log(root: &std::path::Path, session_id: &str, lines: &[&str]) {
        let path = local_session_log::session_log_file_path(root, session_id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, lines.join("\n")).unwrap();
    }

    #[test]
    fn imports_verified_artifacts_from_session_jsonl() {
        let root = std::env::temp_dir().join(format!(
            "elgar-session-log-memory-{}-imports",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_session_log(
            &root,
            "prior-session",
            &[
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-1","action_kind":"CreateFile","operation":"file_written","path":"notes.txt"}}"#,
                "not json",
                r#"{"session_id":"prior-session","turn_index":2,"kind":"action_applied","timestamp_unix_ms":2,"metadata":{"action_id":"action-shell","action_kind":"ShellCommand","operation":"shell_command","command_chars":7}}"#,
            ],
        );
        let session = Session::new("current-session", &root, &root);

        let artifacts = latest_durable_verified_artifacts(&session, 10);

        assert_eq!(artifacts.artifacts.len(), 1);
        assert_eq!(artifacts.artifacts[0].session_id, "prior-session");
        assert_eq!(artifacts.artifacts[0].action_id, "action-1");
        assert!(artifacts.artifacts[0].path.ends_with("notes.txt"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn excludes_current_session_and_caps_imported_artifacts() {
        let root = std::env::temp_dir().join(format!(
            "elgar-session-log-memory-{}-caps",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        write_session_log(
            &root,
            "prior-session",
            &[
                r#"{"session_id":"prior-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":1,"metadata":{"action_id":"action-1","action_kind":"CreateFile","operation":"file_written","path":"one.txt"}}"#,
                r#"{"session_id":"prior-session","turn_index":2,"kind":"action_applied","timestamp_unix_ms":2,"metadata":{"action_id":"action-2","action_kind":"CreateFile","operation":"file_written","path":"two.txt"}}"#,
            ],
        );
        write_session_log(
            &root,
            "current-session",
            &[
                r#"{"session_id":"current-session","turn_index":1,"kind":"action_applied","timestamp_unix_ms":3,"metadata":{"action_id":"action-current","action_kind":"CreateFile","operation":"file_written","path":"current.txt"}}"#,
            ],
        );
        let session = Session::new("current-session", &root, &root);

        let artifacts = latest_durable_verified_artifacts(&session, 1);

        assert_eq!(artifacts.artifacts.len(), 1);
        assert_eq!(artifacts.omitted_count, 1);
        assert_eq!(artifacts.artifacts[0].action_id, "action-2");

        let _ = std::fs::remove_dir_all(&root);
    }
}
