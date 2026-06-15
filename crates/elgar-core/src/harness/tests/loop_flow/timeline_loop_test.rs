//! Verified action timeline behavior in native tool loops.

use std::fs;

use crate::{
    event::ProviderOutput,
    harness::{run_primitive_harness_loop, PermissionMode},
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::{tool_call_output, tool_message_contents};

#[test]
fn primitive_loop_final_round_sees_failed_command_fix_and_success() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-action-timeline-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "write",
            r#"{"path":"src/app/layout.tsx","content":"import \"./globals.css\";\n"}"#,
            "call-write-layout",
        ),
        tool_call_output(
            "bash",
            r#"{"command":"test -f src/app/globals.css"}"#,
            "call-build-fail",
        ),
        tool_call_output(
            "write",
            r#"{"path":"src/app/globals.css","content":"body { margin: 0; }\n"}"#,
            "call-write-globals",
        ),
        tool_call_output(
            "bash",
            r#"{"command":"test -f src/app/globals.css"}"#,
            "call-build-pass",
        ),
        ProviderOutput::new("Created, fixed, and verified."),
    ]);
    let mut session = Session::new("loop-action-timeline-session", &root, &root);
    session.set_permission_mode(PermissionMode::FullAccess);

    let result = run_primitive_harness_loop(&provider, &mut session, "create and verify").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let final_call_messages = calls.last().expect("final provider call");
    let tool_messages = tool_message_contents(final_call_messages);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Created, fixed, and verified.")
    );
    assert!(tool_messages.iter().any(|content| {
        content.contains("VERIFIED_ACTION_TIMELINE")
            && content.contains("contains_failed_command: true")
            && content.contains("bash `test -f src/app/globals.css` exit_code=1 (failed)")
            && content.contains("write `src/app/globals.css`")
            && content.contains("bash `test -f src/app/globals.css` exit_code=0 (passed)")
    }));
}
