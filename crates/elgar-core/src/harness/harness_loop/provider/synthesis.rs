//! Final-answer synthesis for the primitive harness loop.
//!
//! Synthesis is intentionally separate from model-choice. It gives the model
//! verified evidence and asks for an answer without exposing primitive tools.

use std::time::Instant;

use crate::{
    event::{
        Event, ProviderFinished, ProviderStarted, ProviderStreamChunkReceived,
        ProviderStreamTimings,
    },
    harness::harness_loop::provider::{
        synthesis_logs::{
            log_synthesis_canceled, log_synthesis_failed, log_synthesis_finished,
            log_synthesis_started,
        },
        synthesis_stream::log_synthesis_stream_chunk,
    },
    harness::{provider_route::HARNESS_SYNTHESIS_REQUEST_MODE, EvidenceDepth},
    provider::{
        ChatMessage, ControllerProvider, ProviderCancelToken, ProviderErrorKind,
        ProviderStreamChunk,
    },
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
- If a verified command failed and later passed after a write or edit, mention both the failure and the recovery.
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
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
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

    let mut sequence = 0_u64;
    let mut first_reasoning_ms = None;
    let mut first_text_ms = None;
    let mut last_reasoning_ms = None;
    let mut last_text_ms = None;
    let mut last_chunk_ms = None;
    match provider.chat_messages_streaming_with_metadata_cancelable(
        messages,
        &request,
        &mut |chunk| {
            sequence = sequence.saturating_add(1);
            record_stream_timing(
                &chunk,
                started.elapsed().as_millis() as u64,
                &mut first_reasoning_ms,
                &mut first_text_ms,
                &mut last_reasoning_ms,
                &mut last_text_ms,
                &mut last_chunk_ms,
            );
            let event = Event::ProviderStreamChunk(
                ProviderStreamChunkReceived::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    sequence,
                    chunk.clone(),
                )
                .with_context(HARNESS_SYNTHESIS_REQUEST_MODE, "synthesis", 0),
            );
            session.push_event(event.clone());
            stream_events(event);
            log_synthesis_stream_chunk(session, &request.request_id, sequence, &chunk);
        },
        cancel,
    ) {
        Ok(output) => {
            let final_text = output.text.trim().to_string();
            if let Some(metrics) = output.metrics.as_ref() {
                session.record_provider_metrics(metrics);
            }
            let stream_timings = ProviderStreamTimings::from_stream_marks(
                first_reasoning_ms,
                first_text_ms,
                last_reasoning_ms,
                last_text_ms,
                last_chunk_ms,
                started.elapsed().as_millis() as u64,
            );
            session.push_event(Event::ProviderFinished(
                ProviderFinished::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    output.clone(),
                )
                .with_stream_timings(stream_timings.clone()),
            ));
            log_synthesis_finished(
                session,
                started,
                &request.request_id,
                final_text.chars().count(),
                &output,
                &stream_timings,
            );
            Ok(final_text)
        }
        Err(error) => {
            if error.kind == ProviderErrorKind::Canceled {
                log_synthesis_canceled(session, started, &request.request_id);
            }
            log_synthesis_failed(session, started, &request.request_id, &error.to_string());
            Err(error)
        }
    }
}

fn record_stream_timing(
    chunk: &ProviderStreamChunk,
    elapsed_ms: u64,
    first_reasoning_ms: &mut Option<u64>,
    first_text_ms: &mut Option<u64>,
    last_reasoning_ms: &mut Option<u64>,
    last_text_ms: &mut Option<u64>,
    last_chunk_ms: &mut Option<u64>,
) {
    *last_chunk_ms = Some(elapsed_ms);
    match chunk {
        ProviderStreamChunk::Reasoning(_) => {
            if first_reasoning_ms.is_none() {
                *first_reasoning_ms = Some(elapsed_ms);
            }
            *last_reasoning_ms = Some(elapsed_ms);
        }
        ProviderStreamChunk::Text(_) => {
            if first_text_ms.is_none() {
                *first_text_ms = Some(elapsed_ms);
            }
            *last_text_ms = Some(elapsed_ms);
        }
        _ => {}
    }
}
