//! Plain and mixed final-text handling.

use std::fs;

use crate::{event::ProviderOutput, harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::{tool_call_output, tool_message_contents};

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
fn primitive_loop_blocks_unverified_read_claim_without_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-unverified-read-claim-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new("I read package.json successfully."),
        tool_call_output("read", r#"{"path":"package.json"}"#, "call-read-package"),
        ProviderOutput::new("package.json is now verified."),
    ]);
    let mut session = Session::new("loop-unverified-read-claim-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("package.json is now verified.")
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
    assert!(tool_message_contents(&calls[2])
        .iter()
        .any(|content| content.contains("package.json")));
}

#[test]
fn primitive_loop_retries_unverified_local_file_fact_into_tool_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-unverified-local-fact-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new("The app/page.tsx file exports a default component."),
        tool_call_output("read", r#"{"path":"app/page.tsx"}"#, "call-read-page"),
        ProviderOutput::new("app/page.tsx is now verified."),
    ]);
    let mut session = Session::new("loop-unverified-local-fact-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read app/page.tsx").unwrap();

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("app/page.tsx is now verified.")
    );
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
}

#[test]
fn primitive_loop_retries_wrong_read_approval_claim_into_tool_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-read-approval-claim-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("postcss.config.mjs"), "export default {}").unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new(
            "I need your approval to read the `postcss.config.mjs` file. Should I proceed?",
        ),
        tool_call_output(
            "read",
            r#"{"path":"postcss.config.mjs"}"#,
            "call-read-postcss",
        ),
        ProviderOutput::new("postcss.config.mjs is now verified."),
    ]);
    let mut session = Session::new("loop-read-approval-claim-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("postcss.config.mjs is now verified.")
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
    assert!(tool_message_contents(&calls[2])
        .iter()
        .any(|content| content.contains("postcss.config.mjs")));
}

#[test]
fn primitive_loop_stops_after_second_wrong_read_approval_claim() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-second-read-approval-claim-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let approval_text =
        "I need your approval to read the `postcss.config.mjs` file. Should I proceed?";
    let provider = QueuedProvider::new(vec![approval_text, approval_text]);
    let mut session = Session::new("loop-second-read-approval-claim-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "read_only_approval_claim");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Read-only local inspection does not need approval; I need a primitive tool call for verified evidence.")
    );
    assert_eq!(calls.len(), 2);
    assert!(result.rounds.is_empty());
}

#[test]
fn primitive_loop_retries_missing_pending_approval_claim_into_tool_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-missing-pending-approval-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new(
            "Approval is required before executing this command. Please approve `mkdir beta gamma delta` to create the folders.",
        ),
        tool_call_output(
            "bash",
            r#"{"command":"mkdir beta gamma delta"}"#,
            "call-mkdir-beta-gamma-delta",
        ),
        ProviderOutput::new(
            "Approval is required before executing this command. Please approve `mkdir beta gamma delta` to create the folders.",
        ),
    ]);
    let mut session = Session::new("loop-missing-pending-approval-session", &root, &root);

    let result = run_primitive_harness_loop(
        &provider,
        &mut session,
        "Create folders beta, gamma, and delta.",
    )
    .unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some(
            "Approval is required before executing this command. Please approve `mkdir beta gamma delta` to create the folders."
        )
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("bash"));
    let pending = session.pending_approval().expect("pending approval");
    assert_eq!(pending.tool, "bash");
    assert!(pending.arguments_preview.contains("mkdir beta gamma delta"));
    assert!(tool_message_contents(&calls[2]).iter().any(|content| {
        content.contains("VERIFIED_PERMISSION_DECISION")
            && content.contains("tool: bash")
            && content.contains("approval_id: approval-1")
    }));
}

#[test]
fn primitive_loop_stops_after_second_missing_pending_approval_claim() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-second-missing-pending-approval-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let approval_text =
        "Approval is required before executing this command. Please approve `mkdir beta gamma delta`.";
    let provider = QueuedProvider::new(vec![approval_text, approval_text]);
    let mut session = Session::new("loop-second-missing-pending-approval-session", &root, &root);

    let result = run_primitive_harness_loop(
        &provider,
        &mut session,
        "Create folders beta, gamma, and delta.",
    )
    .unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(
        result.stopped_reason,
        "approval_claim_without_pending_approval"
    );
    assert_eq!(
        result.final_text.as_deref(),
        Some(
            "Approval requires a pending harness action; I need a primitive tool call before the user can approve."
        )
    );
    assert_eq!(calls.len(), 2);
    assert!(result.rounds.is_empty());
    assert!(session.pending_approval().is_none());
}

#[test]
fn primitive_loop_stops_after_second_unverified_claim_without_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-second-unverified-claim-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new(vec![
        "I read package.json successfully.",
        "I read package.json successfully.",
    ]);
    let mut session = Session::new("loop-second-unverified-claim-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "unverified_provider_action_claim");
    assert_eq!(
        result.final_text.as_deref(),
        Some("I need verified tool evidence before claiming local project actions or file facts.")
    );
    assert_eq!(calls.len(), 2);
    assert!(result.rounds.is_empty());
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
