//! Tests for building memory from session JSONL events.

use std::io::Write;

use serde_json::json;

use crate::{
    harness::{build_memory_index, read_session_memory_events, HarnessMemoryKind},
    logs::sessions::{self, LocalSessionLogEvent},
};

#[test]
fn reads_missing_session_log_as_empty_memory_events() {
    let root = std::env::temp_dir().join(format!(
        "elgar-memory-missing-session-{}",
        std::process::id()
    ));
    let events = read_session_memory_events(&root, "missing").unwrap();

    assert!(events.is_empty());
}

#[test]
fn builds_memory_index_from_verified_session_events() {
    let root = std::env::temp_dir().join(format!("elgar-memory-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    write_session_event(
        &root,
        "session-1",
        0,
        "harness_tool_result_verified",
        json!({
            "tool": "read",
            "path": "package.json"
        }),
    );
    write_session_event(
        &root,
        "session-1",
        0,
        "harness_tool_result_verified",
        json!({
            "tool": "ls",
            "path": "app"
        }),
    );
    write_session_event(
        &root,
        "session-1",
        0,
        "harness_tool_result_verified",
        json!({
            "tool": "grep",
            "path": ".",
            "query": "tailwind"
        }),
    );
    write_session_event(
        &root,
        "session-1",
        0,
        "harness_approval_decision",
        json!({
            "tool": "write",
            "status": "approved"
        }),
    );

    let events = read_session_memory_events(&root, "session-1").unwrap();
    let index = build_memory_index(&events);

    assert!(index
        .facts_by_kind(HarnessMemoryKind::ReadFile)
        .iter()
        .any(|fact| fact.key == "package.json"));
    assert!(index
        .facts_by_kind(HarnessMemoryKind::ListedDirectory)
        .iter()
        .any(|fact| fact.key == "app"));
    assert!(index
        .facts_by_kind(HarnessMemoryKind::GrepQuery)
        .iter()
        .any(|fact| fact.key == ".:tailwind"));
    assert!(index
        .facts_by_kind(HarnessMemoryKind::PermissionDecision)
        .iter()
        .any(|fact| fact.key == "write"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn memory_index_ignores_provider_prose_events() {
    let root = std::env::temp_dir().join(format!(
        "elgar-memory-ignore-provider-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    write_session_event(
        &root,
        "session-1",
        0,
        "assistant_message",
        json!({
            "text": "I read package.json"
        }),
    );

    let events = read_session_memory_events(&root, "session-1").unwrap();
    let index = build_memory_index(&events);

    assert!(index.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

fn write_session_event(
    project_root: &std::path::Path,
    session_id: &str,
    turn_index: u64,
    kind: &str,
    metadata: serde_json::Value,
) {
    let path = sessions::session_log_path(project_root, session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let event = LocalSessionLogEvent {
        session_id: session_id.to_string(),
        turn_index,
        kind: kind.to_string(),
        timestamp_unix_ms: 1,
        metadata,
    };
    let mut line = serde_json::to_string(&event).unwrap();
    line.push('\n');
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap()
        .write_all(line.as_bytes())
        .unwrap();
}
