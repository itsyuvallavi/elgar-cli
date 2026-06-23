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

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
    let final_text = result.final_text.as_deref().expect("final text");
    assert!(final_text.contains("`postcss.config.mjs`"));
    assert!(final_text.contains("export default {}"));
    assert!(!final_text.contains("Summary"));
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
fn primitive_loop_accepts_contextual_basename_read_and_stops() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-contextual-read-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("Nextjs-1")).unwrap();
    fs::write(root.join("Nextjs-1/package.json"), r#"{"name":"nextjs-1"}"#).unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output(
            "read",
            r#"{"path":"Nextjs-1/package.json"}"#,
            "call-read-context-package",
        ),
        ProviderOutput::new("Nextjs-1/package.json was read."),
    ]);
    let mut session = Session::new("loop-target-contextual-read-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "show me package.json").unwrap();

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
    let final_text = result.final_text.as_deref().expect("final text");
    assert!(final_text.contains("`Nextjs-1/package.json`"));
    assert!(final_text.contains(r#"{"name":"nextjs-1"}"#));
    assert!(!final_text.contains("Summary"));
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("read:Nextjs-1/package.json")
    );
}

#[test]
fn primitive_loop_accepts_contextual_basename_list_and_stops() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-target-contextual-list-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(root.join("Nextjs-1/app")).unwrap();
    fs::write(root.join("Nextjs-1/app/globals.css"), "body {}").unwrap();
    fs::write(
        root.join("Nextjs-1/app/layout.tsx"),
        "export default function Layout() {}",
    )
    .unwrap();
    fs::write(
        root.join("Nextjs-1/app/page.tsx"),
        "export default function Page() {}",
    )
    .unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        tool_call_output("ls", r#"{"path":"Nextjs-1/app"}"#, "call-list-context-app"),
        ProviderOutput::new("Nextjs-1/app was listed."),
    ]);
    let mut session = Session::new("loop-target-contextual-list-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "show me the app folder").unwrap();

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
    let final_text = result.final_text.as_deref().expect("final text");
    assert!(final_text.contains("`Nextjs-1/app`"));
    assert!(final_text.contains("[file] globals.css"));
    assert!(final_text.contains("[file] layout.tsx"));
    assert!(final_text.contains("[file] page.tsx"));
    assert!(!final_text.contains("Summary"));
    assert!(!final_text.contains("Evidence Used"));
    assert!(!final_text.contains("Next Step"));
    assert_eq!(
        result.rounds[0].evidence_label.as_deref(),
        Some("ls:Nextjs-1/app")
    );
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

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
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

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
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
