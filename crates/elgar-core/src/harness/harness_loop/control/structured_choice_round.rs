//! JSON-fallback structured request handling for the primitive harness loop.
//!
//! Native tool calls are preferred. This module handles validated structured
//! model choices that arrive through the fallback parser.

use std::time::Instant;

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            control::{
                native_execution::execute_native_tool_request,
                native_tool_round::NativeToolRoundOutcome,
                synthetic_tool_calls::{
                    synthetic_assistant_tool_call, synthetic_assistant_tool_calls,
                    synthetic_native_tool_request,
                },
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::log_loop_round_finished,
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopRound},
            },
        },
        ModelChoiceTurnError, PrimitiveToolRegistry, ValidatedStructuredRequest,
    },
    provider::{ChatMessage, ControllerProvider, ProviderCancelToken},
    session::Session,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_structured_request_choice<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    request: ValidatedStructuredRequest,
    round_index: usize,
    round_started: Instant,
    registry: &PrimitiveToolRegistry,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    memory: &mut HarnessWorkingMemory,
    rounds: &mut Vec<PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<Evidence>,
    messages: &mut Vec<ChatMessage>,
    loop_turn_id: u64,
    loop_started: Instant,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<NativeToolRoundOutcome, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let synthetic = synthetic_native_tool_request(round_index, 0, request);
    messages.push(synthetic_assistant_tool_call(&synthetic));
    if let Some(result) = execute_native_tool_request(
        provider,
        session,
        input,
        synthetic,
        round_index,
        registry,
        budget,
        budget_state,
        memory,
        rounds,
        evidence,
        messages,
        loop_turn_id,
        loop_started,
        cancel,
        stream_events,
    )? {
        return Ok(NativeToolRoundOutcome::Finish(result));
    }
    log_loop_round_finished(session, round_index, round_started, "evidence_collected");
    Ok(NativeToolRoundOutcome::Continue)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_structured_requests_choice<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    requests: Vec<ValidatedStructuredRequest>,
    round_index: usize,
    round_started: Instant,
    registry: &PrimitiveToolRegistry,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    memory: &mut HarnessWorkingMemory,
    rounds: &mut Vec<PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<Evidence>,
    messages: &mut Vec<ChatMessage>,
    loop_turn_id: u64,
    loop_started: Instant,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<NativeToolRoundOutcome, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let synthetic_requests = requests
        .into_iter()
        .enumerate()
        .map(|(index, request)| synthetic_native_tool_request(round_index, index, request))
        .collect::<Vec<_>>();
    messages.push(synthetic_assistant_tool_calls(&synthetic_requests));
    for request in synthetic_requests {
        if let Some(result) = execute_native_tool_request(
            provider,
            session,
            input,
            request,
            round_index,
            registry,
            budget,
            budget_state,
            memory,
            rounds,
            evidence,
            messages,
            loop_turn_id,
            loop_started,
            cancel,
            stream_events,
        )? {
            return Ok(NativeToolRoundOutcome::Finish(result));
        }
    }
    log_loop_round_finished(
        session,
        round_index,
        round_started,
        "batch_evidence_collected",
    );
    Ok(NativeToolRoundOutcome::Continue)
}
