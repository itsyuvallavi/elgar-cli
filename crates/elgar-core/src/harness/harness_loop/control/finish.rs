//! Finish helpers for the primitive harness loop.
//!
//! Finish code is responsible for turning a stop reason into either a direct
//! model message or a final no-tool synthesis call.

use std::time::Instant;

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            provider::{
                session_context::render_verified_memory_for_session,
                synthesis::run_primitive_loop_synthesis,
            },
            state::{
                logging::log_loop_finished,
                types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
            },
        },
        EvidenceDepth, ModelChoiceTurnError,
    },
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

/// Render the command-line display for one loop result.
pub fn render_primitive_harness_loop_result(result: &PrimitiveHarnessLoopResult) -> String {
    let final_text = result
        .final_text
        .as_deref()
        .unwrap_or("No final model message.");

    format!(
        "harness loop\nevidence items: {}\nstopped: {}\n\nmodel message\n\n{}",
        result.rounds.len(),
        result.stopped_reason,
        final_text
    )
}

/// Finish with a model message that already contains the final answer.
pub(super) fn finish_with_model_message(
    session: &Session,
    content: String,
    rounds: Vec<PrimitiveHarnessLoopRound>,
    stop_reason: String,
    loop_turn_id: u64,
    loop_started: Instant,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError> {
    let result = PrimitiveHarnessLoopResult {
        final_text: Some(content),
        rounds,
        stopped_reason: stop_reason,
    };
    log_loop_finished(session, loop_turn_id, &result, loop_started);
    Ok(result)
}

/// Finish with a no-tool synthesis call over verified evidence.
pub(super) fn synthesize_loop_answer<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    evidence: &[Evidence],
    rounds: Vec<PrimitiveHarnessLoopRound>,
    stop_reason: String,
    evidence_depth: EvidenceDepth,
    loop_turn_id: u64,
    loop_started: Instant,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let evidence_text = render_synthesis_evidence_text(session, evidence);
    let final_text = run_primitive_loop_synthesis(
        provider,
        session,
        input,
        &evidence_text,
        &stop_reason,
        evidence_depth,
        cancel,
        stream_events,
    )
    .map_err(ModelChoiceTurnError::Provider)?;
    let result = PrimitiveHarnessLoopResult {
        final_text: Some(final_text),
        rounds,
        stopped_reason: stop_reason,
    };
    log_loop_finished(session, loop_turn_id, &result, loop_started);
    Ok(result)
}

fn render_synthesis_evidence_text(session: &Session, evidence: &[Evidence]) -> String {
    let current_evidence =
        crate::harness::harness_loop::evidence::render::render_evidence_for_synthesis(evidence);
    if !evidence.is_empty() {
        return current_evidence;
    }

    let memory = render_verified_memory_for_session(session);
    if memory.text.is_empty() {
        return current_evidence;
    }

    format!(
        "{current_evidence}\n\n--- Verified Session Memory Fallback ---\n{}",
        memory.text
    )
}

/// Finish with a validation error before any evidence exists.
pub(super) fn finish_invalid_model_choice(
    session: &Session,
    error: String,
    rounds: Vec<PrimitiveHarnessLoopRound>,
    loop_turn_id: u64,
    loop_started: Instant,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError> {
    let result = PrimitiveHarnessLoopResult {
        final_text: Some(format!(
            "Model returned invalid structured request: {error}"
        )),
        rounds,
        stopped_reason: "invalid_model_choice".to_string(),
    };
    log_loop_finished(session, loop_turn_id, &result, loop_started);
    Ok(result)
}
