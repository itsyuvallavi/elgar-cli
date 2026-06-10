//! Invalid model choice repair paths.

use std::fs;

use crate::{harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;

#[test]
fn primitive_loop_invalid_choice_without_evidence_repairs_to_tool_request() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-invalid-repair-tool-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request""#,
        r#"{"type":"structured_request","kind":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"answer_now","reason":"Package evidence is enough.","evidence_depth":"enough"}"#,
        "Package file reviewed after repair.",
    ]);
    let mut session = Session::new("loop-invalid-repair-tool-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Package file reviewed after repair.")
    );
    assert_eq!(calls.len(), 4);
    assert!(calls[1][1].content.contains("Validation error:"));
    assert!(calls[1][1].content.contains("Invalid response:"));
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
}

#[test]
fn primitive_loop_invalid_choice_without_evidence_repairs_to_plain_answer() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-invalid-repair-answer-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec![r#"{"type":"structured_request""#, "Repaired answer."]);
    let mut session = Session::new("loop-invalid-repair-answer-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "hello").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "model_message");
    assert_eq!(result.final_text.as_deref(), Some("Repaired answer."));
    assert_eq!(calls.len(), 2);
}

#[test]
fn primitive_loop_second_invalid_choice_fails_safely() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-invalid-repair-fails-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request""#,
        r#"{"type":"structured_request""#,
    ]);
    let mut session = Session::new("loop-invalid-repair-fails-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "invalid_model_choice");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Model returned invalid structured request: malformed_json")
    );
    assert_eq!(calls.len(), 2);
}

#[test]
fn primitive_loop_invalid_json_after_evidence_is_final_text() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-invalid-after-evidence-repair-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"structured_request""#,
    ]);
    let mut session = Session::new("loop-invalid-after-evidence-repair-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some(r#"{"type":"structured_request""#)
    );
    assert_eq!(calls.len(), 2);
}

#[test]
fn primitive_loop_answer_now_after_evidence_uses_synthesis() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-invalid-after-evidence-answer-now-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need package metadata.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"answer_now","reason":"Package metadata is enough.","evidence_depth":"limited"}"#,
        "Recovered through answer_now synthesis.",
    ]);
    let mut session = Session::new(
        "loop-invalid-after-evidence-answer-now-session",
        &root,
        &root,
    );

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Recovered through answer_now synthesis.")
    );
    assert_eq!(calls.len(), 3);
}
