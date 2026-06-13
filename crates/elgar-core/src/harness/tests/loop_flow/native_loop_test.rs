//! Native tool-call loop behavior.

use std::fs;

use crate::{
    event::ProviderOutput,
    harness::{run_primitive_harness_loop, MAX_TOOL_CALL_BATCH},
    provider::ChatRole,
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::{tool_call_output, tool_calls_output, tool_message_contents};

#[test]
fn primitive_loop_native_tool_call_appends_tool_result_then_accepts_final_text() {
    let root = std::env::temp_dir().join(format!("elgar-loop-native-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let json_final_text = r#"{
  "name": "nextjs-1",
  "version": "0.1.0"
}"#;
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("read", r#"{"path":"package.json"}"#, "call-read-package"),
        ProviderOutput::new(json_final_text),
    ]);
    let mut session = Session::new("loop-native-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(result.final_text.as_deref(), Some(json_final_text));
    assert_eq!(calls.len(), 2);
    assert!(calls[1]
        .iter()
        .any(|message| matches!(message.role, ChatRole::Assistant)
            && message.tool_calls.len() == 1
            && message.tool_calls[0].id == "call-read-package"));
    assert!(calls[1]
        .iter()
        .any(|message| matches!(message.role, ChatRole::Tool)
            && message.tool_call_id.as_deref() == Some("call-read-package")
            && message.content.contains("package.json")));
}

#[test]
fn primitive_loop_native_tool_batch_appends_all_tool_results() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-batch-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}\n",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_calls_output(vec![
            ("read", r#"{"path":"package.json"}"#, "call-read-package"),
            ("read", r#"{"path":"app/page.tsx"}"#, "call-read-page"),
        ]),
        ProviderOutput::new("Read both requested files."),
    ]);
    let mut session = Session::new("loop-native-batch-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read package and page").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_results = calls[1]
        .iter()
        .filter(|message| matches!(message.role, ChatRole::Tool))
        .collect::<Vec<_>>();

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Read both requested files.")
    );
    assert_eq!(tool_results.len(), 2);
    assert!(tool_results.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-read-package")
            && message.content.contains("package.json")
    }));
    assert!(tool_results.iter().any(|message| {
        message.tool_call_id.as_deref() == Some("call-read-page")
            && message.content.contains("app/page.tsx")
    }));
}

#[test]
fn primitive_loop_native_risky_tool_batch_creates_one_batch_approval() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-risky-batch-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_calls_output(vec![
            (
                "write",
                r#"{"path":"move-a.txt","content":"A"}"#,
                "call-write-a",
            ),
            (
                "write",
                r#"{"path":"move-b.txt","content":"B"}"#,
                "call-write-b",
            ),
        ]),
        ProviderOutput::new("Approval is required for the two writes."),
    ]);
    let mut session = Session::new("loop-native-risky-batch-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "create two files").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_results = tool_message_contents(&calls[1]);
    let pending = session.pending_approval().expect("pending approval");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(pending.tool, "batch");
    assert_eq!(pending.steps.len(), 2);
    assert_eq!(pending.steps[0].tool, "write");
    assert_eq!(pending.steps[1].tool, "write");
    assert!(tool_results
        .iter()
        .any(|content| content.contains("approval_id: approval-1")
            && content.contains("batch_step: 1")));
    assert!(tool_results
        .iter()
        .any(|content| content.contains("approval_id: approval-1")
            && content.contains("batch_step: 2")));
    assert!(!root.join("move-a.txt").exists());
    assert!(!root.join("move-b.txt").exists());
}

#[test]
fn primitive_loop_native_five_write_batch_creates_one_batch_approval() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-five-write-batch-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let tool_calls = (1..=5)
        .map(|index| {
            (
                "write",
                format!(r#"{{"path":"five-{index}.txt","content":"{index}"}}"#),
                format!("call-write-five-{index}"),
            )
        })
        .collect::<Vec<_>>();
    let provider = QueuedProvider::new_outputs(vec![
        tool_calls_output(
            tool_calls
                .iter()
                .map(|(tool, arguments, call_id)| (*tool, arguments.as_str(), call_id.as_str()))
                .collect(),
        ),
        ProviderOutput::new("Approval is required for the five writes."),
    ]);
    let mut session = Session::new("loop-native-five-write-batch-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "create five files").unwrap();
    let pending = session.pending_approval().expect("pending approval");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(pending.tool, "batch");
    assert_eq!(pending.steps.len(), 5);
}

#[test]
fn primitive_loop_native_over_limit_tool_batch_rejects_without_approval() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-over-limit-batch-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let tool_calls = (1..=(MAX_TOOL_CALL_BATCH + 1))
        .map(|index| {
            (
                "write",
                format!(r#"{{"path":"over-{index}.txt","content":"{index}"}}"#),
                format!("call-write-over-{index}"),
            )
        })
        .collect::<Vec<_>>();
    let provider = QueuedProvider::new_outputs(vec![
        tool_calls_output(
            tool_calls
                .iter()
                .map(|(tool, arguments, call_id)| (*tool, arguments.as_str(), call_id.as_str()))
                .collect(),
        ),
        ProviderOutput::new("Rejected over-limit batch without approval."),
    ]);
    let mut session = Session::new("loop-native-over-limit-batch-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "create too many files").unwrap();

    assert_eq!(
        result.stopped_reason,
        format!("too_many_requests:{MAX_TOOL_CALL_BATCH}")
    );
    assert!(session.pending_approval().is_none());
}

#[test]
fn primitive_loop_native_duplicate_request_returns_tool_notice() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-duplicate-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("ls", r#"{"path":"."}"#, "call-list-root"),
        tool_call_output("ls", r#"{"path":"."}"#, "call-list-root-again"),
        ProviderOutput::new("Used the existing listing after duplicate feedback."),
    ]);
    let mut session = Session::new("loop-native-duplicate-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "list root twice").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let tool_messages = tool_message_contents(&calls[2]);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Used the existing listing after duplicate feedback.")
    );
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Duplicate or no-op request rejected")));
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("duplicate:ls:.")
    );
}

#[test]
fn primitive_loop_native_second_duplicate_uses_synthesis_fallback() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-native-duplicate-stop-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("ls", r#"{"path":"."}"#, "call-list-root"),
        tool_call_output("ls", r#"{"path":"."}"#, "call-list-root-again"),
        tool_call_output("ls", r#"{"path":"."}"#, "call-list-root-third"),
        ProviderOutput::new("Stopped duplicate native loop and answered from evidence."),
    ]);
    let mut session = Session::new("loop-native-duplicate-stop-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "list root repeatedly").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "duplicate_loop_detected");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Stopped duplicate native loop and answered from evidence.")
    );
    assert_eq!(calls.len(), 4);
    assert!(calls.last().unwrap()[1]
        .content
        .contains("duplicate_loop_detected"));
}
