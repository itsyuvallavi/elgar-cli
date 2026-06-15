//! Mutation-aware duplicate guard tests.

use std::fs;

use crate::{
    event::ProviderOutput,
    harness::{run_primitive_harness_loop, PermissionMode},
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::{tool_call_output, tool_message_contents};

#[test]
fn primitive_loop_rejects_repeated_bash_without_file_mutation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-repeated-bash-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("bash", r#"{"command":"printf build"}"#, "call-bash-first"),
        tool_call_output("bash", r#"{"command":"printf build"}"#, "call-bash-second"),
        ProviderOutput::new("Used duplicate feedback."),
    ]);
    let mut session = Session::new("loop-repeated-bash-session", &root, &root);
    session.set_permission_mode(PermissionMode::FullAccess);

    let result = run_primitive_harness_loop(&provider, &mut session, "run build twice").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_messages = tool_message_contents(&calls[2]);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Used duplicate feedback.")
    );
    let duplicate_label = result.rounds[1].evidence_label.as_deref().unwrap_or("");
    assert!(duplicate_label.starts_with("duplicate:bash:"));
    assert!(duplicate_label.ends_with(":epoch:0"));
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Duplicate or no-op request rejected")));
}

#[test]
fn primitive_loop_allows_same_bash_after_file_mutation() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-bash-after-edit-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("components.tsx"), "const value = 1;\n").unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "bash",
            r#"{"command":"npm run build 2>&1"}"#,
            "call-build-first",
        ),
        tool_call_output(
            "edit",
            r#"{"path":"components.tsx","old_text":"const value = 1;","new_text":"const value = 2;"}"#,
            "call-edit",
        ),
        tool_call_output(
            "bash",
            r#"{"command":"npm run build 2>&1"}"#,
            "call-build-second",
        ),
        ProviderOutput::new("Build rerun after edit."),
    ]);
    let mut session = Session::new("loop-bash-after-edit-session", &root, &root);
    session.set_permission_mode(PermissionMode::FullAccess);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "build, fix, build again").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let final_tool_messages = tool_message_contents(&calls[3]);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Build rerun after edit.")
    );
    assert!(result.rounds.iter().all(|round| !round
        .evidence_label
        .as_deref()
        .unwrap_or("")
        .starts_with("duplicate:")));
    assert_eq!(
        fs::read_to_string(root.join("components.tsx")).unwrap(),
        "const value = 2;\n"
    );
    assert!(final_tool_messages
        .iter()
        .any(|content| content.contains("VERIFIED_BASH_EXECUTION")));
}

#[test]
fn primitive_loop_resets_duplicate_streak_after_useful_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-duplicate-streak-reset-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("ls", r#"{"path":"."}"#, "call-ls-first"),
        tool_call_output("ls", r#"{"path":"."}"#, "call-ls-duplicate"),
        tool_call_output("find", r#"{"path":".","pattern":"*"}"#, "call-find-first"),
        tool_call_output(
            "find",
            r#"{"path":".","pattern":"*"}"#,
            "call-find-duplicate",
        ),
        ProviderOutput::new("Answered after duplicate feedback."),
    ]);
    let mut session = Session::new("loop-duplicate-streak-reset-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "inspect then summarize").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Answered after duplicate feedback.")
    );
    assert_eq!(calls.len(), 5);
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("duplicate:ls:.")
    );
    assert_eq!(
        result.rounds[3].evidence_label.as_deref(),
        Some("duplicate:find:.:*")
    );
}
