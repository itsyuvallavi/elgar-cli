//! Listing memory and duplicate evidence handling.

use std::fs;

use crate::{harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_message_contents;

#[test]
fn primitive_loop_repeated_evidence_guides_model_to_next_request() {
    let root = std::env::temp_dir().join(format!("elgar-loop-repeat-test-{}", std::process::id()));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::create_dir_all(root.join("components")).unwrap();
    fs::create_dir_all(root.join("public")).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    fs::write(root.join("plan.md"), "# Plan").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing again.","arguments":{"path":"."}}"#,
        r#"{"type":"structured_request","kind":"read","reason":"Need package file.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"answer_now","reason":"Listing and package evidence are enough.","evidence_depth":"enough"}"#,
        "Answered after using existing listing and package file.",
    ]);
    let mut session = Session::new("loop-repeat-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Answered after using existing listing and package file.")
    );
    assert_eq!(calls.len(), 5);
    assert_eq!(result.rounds.len(), 3);
    assert_eq!(result.rounds[0].evidence_label.as_deref(), Some("ls:."));
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("duplicate:ls:.")
    );
    assert_eq!(
        result.rounds[2].evidence_label.as_deref(),
        Some("read:package.json")
    );
    let duplicate_call_tool_messages = tool_message_contents(&calls[2]);
    let read_call_tool_messages = tool_message_contents(&calls[3]);
    assert!(duplicate_call_tool_messages
        .iter()
        .any(|content| content.contains("Read-only directory summary")));
    assert!(duplicate_call_tool_messages
        .iter()
        .any(|content| content.contains("Duplicate or no-op request rejected")));
    assert!(read_call_tool_messages
        .iter()
        .any(|content| content.contains("Read-only project file")));
}

#[test]
fn primitive_loop_listing_memory_is_capped() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-listing-memory-cap-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    for index in 0..10 {
        fs::create_dir_all(root.join(format!("dir{index:02}"))).unwrap();
        fs::write(root.join(format!("file{index:02}.txt")), "demo").unwrap();
    }
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
        r#"{"type":"structured_request","kind":"ls","reason":"Repeat listing.","arguments":{"path":"./"}}"#,
        r#"{"type":"answer_now","reason":"Listing evidence is enough.","evidence_depth":"limited"}"#,
        "Answered from capped listing memory.",
    ]);
    let mut session = Session::new("loop-listing-memory-cap-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_messages = tool_message_contents(&calls[2]);

    assert_eq!(result.stopped_reason, "answer_now");
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Read-only directory summary")));
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Duplicate or no-op request rejected")));
}

#[test]
fn primitive_loop_second_duplicate_stops_with_synthesis() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-duplicate-stop-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing again.","arguments":{"path":"."}}"#,
        r#"{"type":"structured_request","kind":"ls","reason":"Still need listing.","arguments":{"path":"."}}"#,
        "Stopped after duplicate loop and answered from verified evidence.",
    ]);
    let mut session = Session::new("loop-duplicate-stop-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "duplicate_loop_detected");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Stopped after duplicate loop and answered from verified evidence.")
    );
    assert_eq!(calls.len(), 4);
    assert_eq!(result.rounds.len(), 3);
    assert_eq!(result.rounds[0].evidence_label.as_deref(), Some("ls:."));
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("duplicate:ls:.")
    );
    assert_eq!(
        result.rounds[2].evidence_label.as_deref(),
        Some("duplicate:ls:.")
    );
    assert!(calls.last().unwrap()[1]
        .content
        .contains("duplicate_loop_detected"));
}

#[test]
fn primitive_loop_normalizes_duplicate_path_keys() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-normalized-duplicate-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need package.","arguments":{"path":"./package.json"}}"#,
        r#"{"type":"structured_request","kind":"read","reason":"Need package again.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"answer_now","reason":"Package evidence is enough.","evidence_depth":"enough"}"#,
        "Answered after normalized duplicate.",
    ]);
    let mut session = Session::new("loop-normalized-duplicate-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(calls.len(), 4);
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("read:package.json")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("duplicate:read:package.json")
    );
    let tool_messages = tool_message_contents(&calls[2]);
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Read-only project file")));
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Duplicate or no-op request rejected")));
}
