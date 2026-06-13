//! Tool target fidelity loop tests.

use std::fs;

use crate::{event::ProviderOutput, harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::{tool_call_output, tool_message_contents};

#[test]
fn primitive_loop_rejects_wrong_read_target_then_accepts_retry() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-read-retry-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(root.join("postcss.config.mjs"), "export default {}").unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("read", r#"{"path":"app/page.tsx"}"#, "call-read-page"),
        tool_call_output(
            "read",
            r#"{"path":"postcss.config.mjs"}"#,
            "call-read-postcss",
        ),
        ProviderOutput::new("postcss.config.mjs was read."),
    ]);
    let mut session = Session::new("loop-target-read-retry-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("postcss.config.mjs was read.")
    );
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("tool_target_mismatch")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("read:postcss.config.mjs")
    );
    assert!(tool_message_contents(&calls[1])
        .iter()
        .any(|content| content.contains("Tool target mismatch rejected")));
}

#[test]
fn primitive_loop_third_wrong_read_target_stops_with_synthesis() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-read-stop-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("app")).unwrap();
    fs::write(root.join("postcss.config.mjs"), "export default {}").unwrap();
    fs::write(
        root.join("app/page.tsx"),
        "export default function Home() {}",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("read", r#"{"path":"app/page.tsx"}"#, "call-read-page-1"),
        tool_call_output("read", r#"{"path":"app/page.tsx"}"#, "call-read-page-2"),
        tool_call_output("read", r#"{"path":"app/page.tsx"}"#, "call-read-page-3"),
        ProviderOutput::new("Stopped after repeated target mismatch."),
    ]);
    let mut session = Session::new("loop-target-read-stop-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();

    assert_eq!(result.stopped_reason, "tool_target_mismatch");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Stopped after repeated target mismatch.")
    );
    assert_eq!(result.rounds.len(), 3);
    assert!(result
        .rounds
        .iter()
        .all(|round| round.evidence_label.as_deref() == Some("tool_target_mismatch")));
}

#[test]
fn primitive_loop_rejects_wrong_grep_target_then_accepts_file_retry() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-grep-retry-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("tailwind.config.ts"),
        "export default { content: ['./app/**/*.{ts,tsx}'] }\n",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "grep",
            r#"{"path":".","query":"tailwind"}"#,
            "call-grep-root",
        ),
        tool_call_output(
            "grep",
            r#"{"path":"tailwind.config.ts","query":"tailwind"}"#,
            "call-grep-tailwind",
        ),
        ProviderOutput::new("tailwind.config.ts was grepped."),
    ]);
    let mut session = Session::new("loop-target-grep-retry-session", &root, &root);

    let result = run_primitive_harness_loop(
        &provider,
        &mut session,
        "grep tailwind in tailwind.config.ts",
    )
    .unwrap();

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("tool_target_mismatch")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("grep:tailwind.config.ts:tailwind")
    );
}

#[test]
fn primitive_loop_rejects_find_and_root_grep_then_accepts_file_grep() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-grep-second-retry-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("tailwind.config.ts"),
        "export default { content: ['./app/**/*.{ts,tsx}'] }\n",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "find",
            r#"{"path":".","pattern":"*config*"}"#,
            "call-find-config",
        ),
        tool_call_output(
            "grep",
            r#"{"path":".","query":"tailwind"}"#,
            "call-grep-root",
        ),
        tool_call_output(
            "grep",
            r#"{"path":"tailwind.config.ts","query":"tailwind"}"#,
            "call-grep-tailwind",
        ),
        ProviderOutput::new("tailwind.config.ts was grepped."),
    ]);
    let mut session = Session::new("loop-target-grep-second-retry-session", &root, &root);

    let result = run_primitive_harness_loop(
        &provider,
        &mut session,
        "search for tailwind in tailwind.config.ts",
    )
    .unwrap();

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("tool_target_mismatch")
    );
    assert_eq!(
        result.rounds[1].evidence_label.as_deref(),
        Some("tool_target_mismatch")
    );
    assert_eq!(
        result.rounds[2].evidence_label.as_deref(),
        Some("grep:tailwind.config.ts:tailwind")
    );
}
