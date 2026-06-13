//! Final-answer synthesis for the primitive harness loop.
//!
//! Synthesis is intentionally separate from model-choice. It gives the model
//! verified evidence and asks for an answer without exposing primitive tools.

use std::time::Instant;

use serde_json::json;

use crate::{
    event::{Event, ProviderFinished, ProviderOutput, ProviderStarted},
    harness::{provider_route::HARNESS_SYNTHESIS_REQUEST_MODE, EvidenceDepth},
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::{ChatMessage, ControllerProvider},
    session::Session,
};

const SYNTHESIS_PROMPT: &str = r#"You are writing the final answer for Elgar's primitive harness loop.

Do not request tools, files, shell commands, or permissions.
Use only the verified evidence supplied in this request.
Do not claim file contents were read unless the evidence says they were.
Do not claim commands ran or files changed.
Be concise and organized unless the user asked for depth.

When answering from verified evidence:
- Say what was actually verified.
- Reference evidence labels or file paths when useful.
- Separate verified facts from reasonable inferences.
- If evidence is shallow, say the review is shallow.
- Give concrete next steps in priority order when useful.
- Do not claim a deep review if only structure or config was inspected.

Use short sections when useful:
- Summary
- Evidence Used
- Findings
- Next Step"#;

/// Ask the provider for a final answer with no tools exposed.
pub(in crate::harness::harness_loop) fn run_primitive_loop_synthesis<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    evidence_text: &str,
    stop_reason: &str,
    evidence_depth: EvidenceDepth,
) -> Result<String, crate::provider::ProviderError>
where
    P: ControllerProvider,
{
    let started = Instant::now();
    let request = provider.request_metadata_for_mode(HARNESS_SYNTHESIS_REQUEST_MODE);
    let profile = request.profile.as_ref();
    log_synthesis_started(
        session,
        &request.request_id,
        stop_reason,
        evidence_depth,
        evidence_text.len(),
    );
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), HARNESS_SYNTHESIS_REQUEST_MODE, 0)
            .with_provider_profile(
                profile.map(|profile| profile.backend),
                profile.and_then(|profile| profile.reasoning),
                profile.and_then(|profile| profile.context_length),
                profile.and_then(|profile| profile.stats),
            ),
    ));

    let messages = vec![
        ChatMessage::system(SYNTHESIS_PROMPT),
        ChatMessage::user(format!(
            "Original user request:\n{}\n\nStop reason:\n{}\n\nEvidence depth:\n{}\n\nVerified evidence:\n{}",
            input.trim(),
            stop_reason,
            evidence_depth.as_str(),
            evidence_text
        )),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let final_text = output.text.trim().to_string();
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            session.push_event(Event::ProviderFinished(ProviderFinished::new(
                request.provider.clone(),
                request.request_id.clone(),
                output.clone(),
            )));
            log_synthesis_finished(
                session,
                started,
                &request.request_id,
                final_text.chars().count(),
                &output,
            );
            Ok(final_text)
        }
        Err(error) => {
            log_synthesis_failed(session, started, &request.request_id, &error.to_string());
            Err(error)
        }
    }
}

fn log_synthesis_started(
    session: &Session,
    request_id: &str,
    stop_reason: &str,
    evidence_depth: EvidenceDepth,
    evidence_bytes: usize,
) {
    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "stop_reason": stop_reason,
        "evidence_depth": evidence_depth.as_str(),
        "evidence_mode": "full_verified",
        "evidence_bytes": evidence_bytes
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_loop_synthesis",
            "harness_loop_synthesis_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_synthesis_started", metadata);
}

fn log_synthesis_finished(
    session: &Session,
    started: Instant,
    request_id: &str,
    response_chars: usize,
    output: &ProviderOutput,
) {
    let usage = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.usage.as_ref());
    let backend = output
        .metrics
        .as_ref()
        .and_then(|metrics| metrics.backend)
        .map(|backend| format!("{backend:?}"));

    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "response_chars": response_chars,
        "backend": backend,
        "provider_response_has_thinking": output.has_thinking(),
        "provider_response_thinking_chars": output.thinking_chars(),
        "prompt_tokens": usage.and_then(|usage| usage.prompt_tokens),
        "completion_tokens": usage.and_then(|usage| usage.completion_tokens),
        "total_tokens": usage.and_then(|usage| usage.total_tokens)
    });
    let duration_ms = started.elapsed().as_millis() as u64;
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_loop_synthesis",
            "harness_loop_synthesis_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    let mut session_metadata = metadata;
    if let Some(object) = session_metadata.as_object_mut() {
        object.insert("duration_ms".to_string(), json!(duration_ms));
    }
    session.log_harness_event("harness_synthesis_finished", session_metadata);
}

fn log_synthesis_failed(session: &Session, started: Instant, request_id: &str, error: &str) {
    let metadata = json!({
        "request_mode": HARNESS_SYNTHESIS_REQUEST_MODE,
        "request_id": request_id,
        "error": error
    });
    let duration_ms = started.elapsed().as_millis() as u64;
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_loop_synthesis",
            "harness_loop_synthesis_failed",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    let mut session_metadata = metadata;
    if let Some(object) = session_metadata.as_object_mut() {
        object.insert("duration_ms".to_string(), json!(duration_ms));
    }
    session.log_harness_event("harness_synthesis_failed", session_metadata);
}
