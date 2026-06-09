//! Primitive harness loop behavior tests.

use std::fs;

use crate::{
    event::ProviderOutput,
    harness::run_primitive_harness_loop,
    provider::{ChatMessage, ChatRole, ChatToolCall, ChatToolCallFunction},
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;

fn tool_call_output(name: &str, arguments: &str, id: &str) -> ProviderOutput {
    tool_calls_output(vec![(name, arguments, id)])
}

fn tool_calls_output(calls: Vec<(&str, &str, &str)>) -> ProviderOutput {
    ProviderOutput::new("").with_tool_calls(
        calls
            .into_iter()
            .map(|(name, arguments, id)| ChatToolCall {
                id: id.to_string(),
                tool_type: "function".to_string(),
                function: ChatToolCallFunction {
                    name: name.to_string(),
                    arguments: arguments.to_string(),
                },
            })
            .collect(),
    )
}

fn tool_message_contents(messages: &[ChatMessage]) -> Vec<&str> {
    messages
        .iter()
        .filter(|message| matches!(message.role, ChatRole::Tool))
        .map(|message| message.content.as_str())
        .collect()
}

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
    let tool_messages = tool_message_contents(&calls[1]);

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Shell execution is not enabled yet.")
    );
    assert!(tool_messages.iter().any(|content| {
        content.contains("VERIFIED_PERMISSION_DECISION")
            && content.contains("tool: bash")
            && content.contains("decision: needs_approval")
            && content.contains("execution_performed: false")
    }));
}

#[test]
fn primitive_loop_native_risky_tool_calls_return_permission_tool_results() {
    for (tool, arguments) in [
        ("bash", r#"{"command":"echo hello"}"#),
        ("write", r#"{"path":"demo.txt","content":"hello"}"#),
        ("edit", r#"{"path":"demo.txt","patch":"replace hello"}"#),
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
        let expected_final = format!("{tool} needs approval.");

        let result = run_primitive_harness_loop(&provider, &mut session, "do risky work").unwrap();
        let calls = provider.calls.lock().expect("calls lock");
        let tool_messages = tool_message_contents(&calls[1]);

        assert_eq!(result.stopped_reason, "native_final_text");
        assert_eq!(result.final_text.as_deref(), Some(expected_final.as_str()));
        assert!(tool_messages.iter().any(|content| {
            content.contains("VERIFIED_PERMISSION_DECISION")
                && content.contains(&format!("tool: {tool}"))
                && content.contains("decision: needs_approval")
                && content.contains("execution_performed: false")
        }));
    }
}

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

#[test]
fn primitive_loop_answer_now_switches_to_synthesis() {
    let root =
        std::env::temp_dir().join(format!("elgar-loop-answer-now-test-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"ls","reason":"Need listing.","arguments":{"path":"."}}"#,
        r#"{"type":"answer_now","reason":"Project tree is enough for a limited review.","evidence_depth":"limited"}"#,
        "Final review from synthesis.",
    ]);
    let mut session = Session::new("loop-answer-now-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let second_decision_call = &calls[1];
    let synthesis_call = calls.last().expect("synthesis call");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Final review from synthesis.")
    );
    assert_eq!(calls.len(), 3);
    assert!(tool_message_contents(second_decision_call)
        .iter()
        .any(|content| content.contains("Read-only directory summary selected by Elgar harness.")));
    assert!(synthesis_call[1].content.contains("answer_now"));
    assert!(synthesis_call[1]
        .content
        .contains("Evidence depth:\nlimited"));
    assert!(synthesis_call[1]
        .content
        .contains("Read-only directory summary selected by Elgar harness."));
    assert!(synthesis_call[0]
        .content
        .contains("Say what was actually verified"));
    assert!(synthesis_call[0].content.contains("If evidence is shallow"));
    assert!(!synthesis_call[0]
        .content
        .contains("Available primitive tools"));
    assert!(!synthesis_call[0].content.contains("review_project"));
    assert!(!synthesis_call[0].content.contains("package.json"));
}

#[test]
fn primitive_loop_decisions_use_compact_evidence_but_synthesis_gets_full_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-compact-evidence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let mut contents = String::new();
    for index in 0..140 {
        contents.push_str(&format!("line-{index}: important project detail\n"));
    }
    fs::write(root.join("large.txt"), contents).unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_request","kind":"read","reason":"Need large file.","arguments":{"path":"large.txt"}}"#,
        r#"{"type":"answer_now","reason":"Large file evidence is enough."}"#,
        "Final answer from full evidence.",
    ]);
    let mut session = Session::new("loop-compact-evidence-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review large.txt").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let second_decision_call = &calls[1];
    let synthesis_call = calls.last().expect("synthesis call");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Final answer from full evidence.")
    );
    assert!(tool_message_contents(second_decision_call)
        .iter()
        .any(|content| content.contains("Read-only project file")));
    assert!(synthesis_call[1].content.contains("Verified evidence"));
    assert!(synthesis_call[1].content.contains("line-139"));
}

#[test]
fn primitive_loop_executes_structured_request_batch() {
    let root = std::env::temp_dir().join(format!("elgar-loop-batch-test-{}", std::process::id()));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Page() {}",
    )
    .unwrap();
    fs::write(
        root.join("app/layout.tsx"),
        "export default function Layout() {}",
    )
    .unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_requests","reason":"Need app files.","requests":[{"kind":"ls","arguments":{"path":"app"}},{"kind":"read","arguments":{"path":"app/page.tsx"}},{"kind":"read","arguments":{"path":"app/layout.tsx"}}]}"#,
        r#"{"type":"answer_now","reason":"App directory files are enough.","evidence_depth":"enough"}"#,
        "The app directory was read.",
    ]);
    let mut session = Session::new("loop-batch-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "read app").unwrap();
    let calls = provider.calls.lock().expect("calls lock");
    let second_decision_call = &calls[1];

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("The app directory was read.")
    );
    assert_eq!(result.rounds.len(), 3);
    assert_eq!(result.rounds[0].round_index, 0);
    assert_eq!(result.rounds[1].round_index, 0);
    assert_eq!(result.rounds[2].round_index, 0);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("ls"));
    assert_eq!(result.rounds[1].tool.as_deref(), Some("read"));
    assert_eq!(result.rounds[2].tool.as_deref(), Some("read"));
    assert_eq!(calls.len(), 3);
    let tool_messages = tool_message_contents(second_decision_call);
    assert_eq!(tool_messages.len(), 3);
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("Read-only directory summary")));
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("app/page.tsx")));
    assert!(tool_messages
        .iter()
        .any(|content| content.contains("app/layout.tsx")));
}

#[test]
fn primitive_loop_batch_repeat_adds_notice_and_continues_batch() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-batch-repeat-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_requests","reason":"Need duplicate listing and package.","requests":[{"kind":"ls","arguments":{"path":"."}},{"kind":"ls","arguments":{"path":"."}},{"kind":"read","arguments":{"path":"package.json"}}]}"#,
        r#"{"type":"answer_now","reason":"Repeated notice and package evidence are enough.","evidence_depth":"limited"}"#,
        "Answered after repeated evidence notice.",
    ]);
    let mut session = Session::new("loop-batch-repeat-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Answered after repeated evidence notice.")
    );
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
    assert_eq!(calls.len(), 3);
}

#[test]
fn primitive_loop_batch_executes_all_requested_evidence_without_content_budget() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-batch-no-content-budget-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::create_dir_all(root.join("components")).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    fs::write(root.join("README.md"), "# Demo").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_requests","reason":"Need broad file checks.","requests":[{"kind":"find","arguments":{"path":".","pattern":"package.json"}},{"kind":"find","arguments":{"path":".","pattern":"README*"}},{"kind":"find","arguments":{"path":".","pattern":"app"}},{"kind":"find","arguments":{"path":".","pattern":"components"}}]}"#,
        r#"{"type":"structured_request","kind":"read","reason":"Read package after broad finds.","arguments":{"path":"package.json"}}"#,
        r#"{"type":"answer_now","reason":"Find and package evidence are enough.","evidence_depth":"limited"}"#,
        "Answered after full batch and read.",
    ]);
    let mut session = Session::new("loop-batch-no-content-budget-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Answered after full batch and read.")
    );
    assert_eq!(calls.len(), 4);
    assert_eq!(result.rounds.len(), 5);
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("find:.:package.json")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("find:.:README*")
    );
    assert_eq!(
        result.rounds[2].evidence_label.as_deref(),
        Some("find:.:app")
    );
    assert_eq!(
        result.rounds[3].evidence_label.as_deref(),
        Some("find:.:components")
    );
    assert_eq!(
        result.rounds[4].evidence_label.as_deref(),
        Some("read:package.json")
    );
}

#[test]
fn primitive_loop_continues_after_large_find_batch_without_content_budget() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-large-find-batch-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::create_dir_all(root.join("components")).unwrap();
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    fs::write(root.join("README.md"), "# Demo").unwrap();
    let provider = QueuedProvider::new(vec![
        r#"{"type":"structured_requests","reason":"Use broad find evidence.","requests":[{"kind":"find","arguments":{"path":".","pattern":"package.json"}},{"kind":"find","arguments":{"path":".","pattern":"README*"}},{"kind":"find","arguments":{"path":".","pattern":"app"}},{"kind":"find","arguments":{"path":".","pattern":"components"}}]}"#,
        r#"{"type":"answer_now","reason":"All requested find evidence is enough.","evidence_depth":"limited"}"#,
        "Answered after large find batch.",
    ]);
    let mut session = Session::new("loop-large-find-batch-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "review project").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "answer_now");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Answered after large find batch.")
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(result.rounds.len(), 4);
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("find:.:package.json")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("find:.:README*")
    );
    assert_eq!(
        result.rounds[2].evidence_label.as_deref(),
        Some("find:.:app")
    );
    assert_eq!(
        result.rounds[3].evidence_label.as_deref(),
        Some("find:.:components")
    );
}

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
