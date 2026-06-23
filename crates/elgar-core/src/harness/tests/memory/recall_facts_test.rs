//! Tests for verified-fact injection into harness prompts.

use std::io::Write;

use serde_json::json;

use crate::{
    event::AssistantMessage,
    harness::run_primitive_harness_loop,
    logs::sessions::{self, LocalSessionLogEvent},
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;

#[test]
fn recall_turn_includes_verified_session_facts_without_tools() {
    let root = std::env::temp_dir().join(format!("elgar-recall-facts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let session_id = "recall-session";
    write_harness_event(
        &root,
        session_id,
        0,
        "harness_tool_result_verified",
        json!({ "tool": "read", "path": "package.json" }),
    );
    write_harness_event(
        &root,
        session_id,
        1,
        "harness_tool_result_verified",
        json!({ "tool": "ls", "path": "app" }),
    );
    write_harness_event(
        &root,
        session_id,
        2,
        "harness_write_execution_finished",
        json!({ "tool": "write", "path": "mem-audit.md", "exit_code": 0 }),
    );

    let provider = QueuedProvider::new(vec![
        "package.json, app, and mem-audit.md were touched earlier in this session.",
    ]);
    let mut session = Session::new(session_id, &root, &root);
    session.push_event(crate::event::Event::UserMessage(
        crate::event::UserMessage::new("read package.json"),
    ));
    session.push_event(crate::event::Event::AssistantMessage(
        AssistantMessage::new(
            "Read package.json.",
            crate::event::AssistantMessageSource::Provider,
        ),
    ));

    let result = run_primitive_harness_loop(
        &provider,
        &mut session,
        "what exact files did you read, list, and write in THIS conversation? no tools.",
    )
    .unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let system = &calls[0][0].content;

    assert!(system.contains("package.json"));
    assert!(system.contains("app"));
    assert!(system.contains("write:mem-audit.md") || system.contains("mem-audit.md"));
    assert_eq!(
        result.final_text.as_deref(),
        Some("package.json, app, and mem-audit.md were touched earlier in this session.")
    );
    assert!(calls[0].len() >= 2);

    let _ = std::fs::remove_dir_all(root);
}

fn write_harness_event(
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
