//! Empty provider response recovery behavior.

use std::fs;

use crate::{
    event::ProviderOutput,
    harness::{run_primitive_harness_loop, ModelChoiceTurnError},
    provider::{ProviderError, ProviderErrorKind},
    session::Session,
};

use super::super::support::queued_provider::QueuedProvider;
use super::loop_helpers::tool_call_output;

fn empty_response_error() -> ProviderError {
    ProviderError::empty_response("provider response contained no text or tool calls")
}

#[test]
fn primitive_loop_retries_empty_response_before_evidence_then_succeeds() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-empty-before-evidence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_results(vec![
        Err(empty_response_error()),
        Ok(ProviderOutput::new("Recovered after an empty response.")),
    ]);
    let mut session = Session::new("loop-empty-before-evidence-session", &root, &root);

    let result = run_primitive_harness_loop(&provider, &mut session, "hello").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "model_message");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Recovered after an empty response.")
    );
    assert_eq!(calls.len(), 2);
}

#[test]
fn primitive_loop_retries_empty_response_after_evidence_then_succeeds() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-empty-after-evidence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new_results(vec![
        Ok(tool_call_output(
            "read",
            r#"{"path":"package.json"}"#,
            "call-read-package",
        )),
        Err(empty_response_error()),
        Ok(ProviderOutput::new("package.json is verified.")),
    ]);
    let mut session = Session::new("loop-empty-after-evidence-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "show me package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "native_final_text");
    assert_eq!(
        result.final_text.as_deref(),
        Some("package.json is verified.")
    );
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(result.rounds[0].tool.as_deref(), Some("read"));
    assert_eq!(calls.len(), 3);
}

#[test]
fn primitive_loop_synthesizes_after_repeated_empty_response_with_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-empty-synthesis-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("package.json"), "{}").unwrap();
    let provider = QueuedProvider::new_results(vec![
        Ok(tool_call_output(
            "read",
            r#"{"path":"package.json"}"#,
            "call-read-package",
        )),
        Err(empty_response_error()),
        Err(empty_response_error()),
        Ok(ProviderOutput::new(
            "Synthesized answer from verified package.json evidence.",
        )),
    ]);
    let mut session = Session::new("loop-empty-synthesis-session", &root, &root);

    let result =
        run_primitive_harness_loop(&provider, &mut session, "show me package.json").unwrap();
    let calls = provider.calls.lock().expect("calls lock");

    assert_eq!(result.stopped_reason, "empty_provider_response_synthesis");
    assert_eq!(
        result.final_text.as_deref(),
        Some("Synthesized answer from verified package.json evidence.")
    );
    assert_eq!(result.rounds.len(), 1);
    assert_eq!(calls.len(), 4);
}

#[test]
fn primitive_loop_returns_provider_error_after_repeated_empty_response_without_evidence() {
    let root = std::env::temp_dir().join(format!(
        "elgar-loop-empty-no-evidence-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let provider = QueuedProvider::new_results(vec![
        Err(empty_response_error()),
        Err(empty_response_error()),
    ]);
    let mut session = Session::new("loop-empty-no-evidence-session", &root, &root);

    let error = run_primitive_harness_loop(&provider, &mut session, "hello").unwrap_err();
    let calls = provider.calls.lock().expect("calls lock");

    assert!(matches!(
        error,
        ModelChoiceTurnError::Provider(ProviderError {
            kind: ProviderErrorKind::EmptyResponse,
            ..
        })
    ));
    assert_eq!(calls.len(), 2);
}
