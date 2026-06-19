//! Direct primitive no-evidence guard tests.

use std::fs;

use crate::{event::ProviderOutput, harness::run_primitive_harness_loop, session::Session};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_call_output;

#[test]
fn primitive_loop_retries_direct_read_missing_file_claim_into_tool_call() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-direct-read-missing-claim-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("postcss.config.mjs"), "export default {}").unwrap();
    let provider = QueuedProvider::new_outputs(vec![
        ProviderOutput::new(
            "I cannot read `postcss.config.mjs` because it does not exist in the current project.",
        ),
        tool_call_output(
            "read",
            r#"{"path":"postcss.config.mjs"}"#,
            "call-read-postcss",
        ),
        ProviderOutput::new("postcss.config.mjs is now verified."),
    ]);
    let mut session = Session::new("loop-direct-read-missing-claim-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "direct_evidence_satisfied");
    assert_eq!(
        result.final_text.as_deref(),
        Some("postcss.config.mjs is now verified.")
    );
    assert_eq!(calls.len(), 3);
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
    assert!(calls[2]
        .iter()
        .any(|message| message.content.contains("postcss.config.mjs")));
}

#[test]
fn primitive_loop_stops_after_second_direct_read_without_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-direct-read-missing-stop-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let missing_text =
        "I cannot read `postcss.config.mjs` because it does not exist in the current project.";
    let provider = QueuedProvider::new(vec![missing_text, missing_text]);
    let mut session = Session::new("loop-direct-read-missing-stop-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "read postcss.config.mjs").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "missing_primitive_evidence");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Direct primitive requests need verified tool evidence before answering.")
    );
    assert_eq!(calls.len(), 2);
    assert!(result.rounds.is_empty());
}
