//! Execution failure tool results.

use std::fs;

use crate::{harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_message_contents;

#[test]
fn primitive_loop_execution_failure_becomes_tool_result_then_final_text() {
    let root = std::env::temp_dir().join(format!("elgar-loop-error-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need missing file.","arguments":{"path":"missing.txt"}}"#,
        "Could not inspect the missing file.",
    ]);
    let mut session = Session::new("loop-error-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review missing.txt").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Could not inspect the missing file.")
    );
    assert_eq!(calls.len(), 2);
    assert!(tool_message_contents(calls.last().expect("second call"))
        .iter()
        .any(|content| content.contains("VERIFIED_EXECUTION_ERROR")));
}
