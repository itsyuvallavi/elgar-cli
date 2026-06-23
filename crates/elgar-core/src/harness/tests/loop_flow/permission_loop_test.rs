//! Risky primitive permission tool results.

use std::fs;

use crate::{event::ProviderOutput, harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_call_output;

#[test]
fn primitive_loop_risky_json_fallback_returns_permission_tool_result() {
    let root =
        std::env::temp_dir().join(format!("elgar-loop-permission-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new(
            r#"{"type":"structured_request","kind":"bash","reason":"try shell","arguments":{"command":"echo hello"}}"#,
        ),
        ProviderOutput::new("Shell execution is not enabled yet."),
    ]);
    let mut session = Session::new("loop-permission-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "run echo hello").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "approval_pending");
    assert_eq!(calls.len(), 1);
    assert!(result
        .final_text
        .as_deref()
        .is_some_and(|text| text.contains("bash")));
    assert!(result.rounds[0]
        .evidence_label
        .as_deref()
        .is_some_and(|label| label.starts_with("bash:")));
    let pending = session.pending_approval().expect("pending approval");
    assert_eq!(pending.id, "approval-1");
    assert_eq!(pending.tool, "bash");
    assert_eq!(pending.status.as_str(), "pending");
    assert!(pending.arguments_preview.contains("echo hello"));
}

#[test]
fn primitive_loop_native_risky_tool_calls_return_permission_tool_results() {
    for (tool, arguments) in [
        ("bash", r#"{"command":"echo hello"}"#),
        ("write", r#"{"path":"demo.txt","content":"hello"}"#),
        (
            "edit",
            r#"{"path":"demo.txt","old_text":"hello","new_text":"goodbye"}"#,
        ),
    ] {
        let root = std::env::temp_dir().join(format!(
            "elgar-loop-native-permission-{tool}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let provider = QueuedProvider::new_outputs(vec![
            tool_call_output(tool, arguments, &format!("call-{tool}")),
            ProviderOutput::new(format!("{tool} needs approval.")),
        ]);
        let mut session = Session::new(format!("loop-native-permission-{tool}"), &root, &root);

        let result = run_primitive_harness_loop(&provider, &mut session, "do risky work").unwrap();
        let calls = provider.calls.lock().expect("calls lock");

        assert_eq!(result.stopped_reason, "approval_pending");
        assert_eq!(calls.len(), 1);
        assert!(result
            .final_text
            .as_deref()
            .is_some_and(|text| text.contains(tool)));
        let pending = session.pending_approval().expect("pending approval");
        assert_eq!(pending.id, "approval-1");
        assert_eq!(pending.tool, tool);
        assert_eq!(pending.status.as_str(), "pending");
    }
}
