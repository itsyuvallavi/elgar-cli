//! Plain and mixed final-text handling.

use std::fs;

use crate::{harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_message_contents;

#[test]
fn primitive_loop_plain_message_without_evidence_is_final_answer() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-plain-message-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec!["This is a direct model answer."]);
    let mut session = Session::new("loop-plain-message-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "model_message");
    assert_eq!(
        result.final_text.as_deref(),
        Some("This is a direct model answer.")
    );
    assert_eq!(calls.len(), 1);
}

#[test]
fn primitive_loop_plain_message_after_evidence_is_final_text() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-plain-after-evidence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
        "I first need to understand the package metadata.",
    ]);
    let mut session = Session::new("loop-plain-after-evidence-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("I first need to understand the package metadata.")
    );
    assert_eq!(calls.len(), 2);
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
    assert!(tool_message_contents(&calls[1])
        .iter()
        .any(|content| content.contains("package.json")));
}

#[test]
fn primitive_loop_mixed_message_after_evidence_is_final_text() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-mixed-message-tool-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
        r#"I need one more file.
{"type":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
    ]);
    let mut session = Session::new("loop-mixed-message-tool-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some(
            r#"I need one more file.
{"type":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#
        )
    );
    assert_eq!(calls.len(), 2);
}
