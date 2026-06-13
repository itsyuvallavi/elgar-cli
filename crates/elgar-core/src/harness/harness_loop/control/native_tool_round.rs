//! Native tool-call round handling for the primitive harness loop.
//!
//! This module owns one branch of the loop: provider-native tool calls. It
//! parses tool calls, appends tool-call messages, executes validated primitive
//! requests, and reports whether the loop should continue or finish.

use std::time::Instant;

use crate::{
    event::ProviderOutput,
    harness::{
        harness_loop::{
            control::{
                choice_from_output::native_tool_requests_from_provider_output,
                finish::synthesize_loop_answer, native_execution::execute_native_tool_request,
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::log_loop_round_finished,
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
            },
        },
        EvidenceDepth, ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ControllerProvider},
    session::Session,
};

pub(super) enum NativeToolRoundOutcome {
    Continue,
    Finish(PrimitiveHarnessLoopResult),
}

pub(super) fn handle_native_tool_output<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    output: &ProviderOutput,
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
) -> Result<NativeToolRoundOutcome, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let native_requests = match native_tool_requests_from_provider_output(output, registry) {
        Ok(requests) => requests,
        Err(error) => {
            let result = synthesize_loop_answer(
                provider,
                session,
                input,
                evidence,
                rounds.clone(),
                error.as_str(),
                EvidenceDepth::Limited,
                loop_turn_id,
                loop_started,
            )?;
            return Ok(NativeToolRoundOutcome::Finish(result));
        }
    };

    messages.push(
        ChatMessage::assistant(output.text.clone()).with_tool_calls(output.tool_calls.clone()),
    );

    for native_request in native_requests {
        if let Some(result) = execute_native_tool_request(
            provider,
            session,
            input,
            native_request,
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
        )? {
            return Ok(NativeToolRoundOutcome::Finish(result));
        }
    }

    budget_state.decision_calls += 1;
    log_loop_round_finished(
        session,
        round_index,
        round_started,
        "native_tool_results_collected",
    );

    Ok(NativeToolRoundOutcome::Continue)
}
