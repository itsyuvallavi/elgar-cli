//! Provider error recovery for the primitive harness loop.
//!
//! This module keeps transient provider failures out of the coordinator while
//! preserving the same verified-evidence finish rules as normal loop exits.

use std::time::Instant;

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            control::finish::synthesize_loop_answer,
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::log_loop_round_finished,
                types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
            },
        },
        EvidenceDepth, ModelChoiceTurnError,
    },
    provider::{ChatMessage, ControllerProvider, ProviderCancelToken, ProviderErrorKind},
    session::Session,
};

pub(super) enum ProviderLoopErrorOutcome {
    Retry {
        returned_rounds: Vec<PrimitiveHarnessLoopRound>,
    },
    Finish(PrimitiveHarnessLoopResult),
}

pub(super) fn handle_provider_loop_error<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    error: ModelChoiceTurnError,
    round_index: usize,
    round_started: Instant,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    evidence: &[Evidence],
    messages: &mut Vec<ChatMessage>,
    rounds: Vec<PrimitiveHarnessLoopRound>,
    loop_turn_id: u64,
    loop_started: Instant,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<ProviderLoopErrorOutcome, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let ModelChoiceTurnError::Provider(provider_error) = error else {
        return Err(error);
    };
    if provider_error.kind != ProviderErrorKind::EmptyResponse {
        return Err(ModelChoiceTurnError::Provider(provider_error));
    }

    if budget_state.empty_provider_response_retries < budget.max_empty_provider_response_retries {
        budget_state.empty_provider_response_retries += 1;
        messages.push(ChatMessage::system(
            "RUNTIME VALIDATION: The previous provider response was empty. Return either a valid tool call or final text grounded in verified evidence.",
        ));
        log_loop_round_finished(
            session,
            round_index,
            round_started,
            "empty_provider_response_retry",
        );
        return Ok(ProviderLoopErrorOutcome::Retry {
            returned_rounds: rounds,
        });
    }

    if !evidence.is_empty() {
        log_loop_round_finished(
            session,
            round_index,
            round_started,
            "empty_provider_response_synthesis",
        );
        let result = synthesize_loop_answer(
            provider,
            session,
            input,
            evidence,
            rounds,
            "empty_provider_response_synthesis".to_string(),
            EvidenceDepth::Limited,
            loop_turn_id,
            loop_started,
            cancel,
            stream_events,
        )?;
        return Ok(ProviderLoopErrorOutcome::Finish(result));
    }

    log_loop_round_finished(
        session,
        round_index,
        round_started,
        "empty_provider_response_error",
    );
    Err(ModelChoiceTurnError::Provider(provider_error))
}
