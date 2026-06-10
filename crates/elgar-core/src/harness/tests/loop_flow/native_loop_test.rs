//! Native tool-call loop behavior.

use std::fs;

use crate::{
    event::ProviderOutput, harness::run_primitive_harness_loop, provider::ChatRole,
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
