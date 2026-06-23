//! Structured request batches and synthesis triggers.

use std::fs;

use crate::{harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_message_contents;

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
