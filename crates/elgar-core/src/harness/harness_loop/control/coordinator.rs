//! Coordinator for the primitive harness loop.
//!
//! This file owns loop order only: ask the model what evidence it wants,
//! collect verified evidence, and choose the stop path. Parsing, execution,
//! provider calls, and finishing live in sibling control/provider modules.

use std::time::Instant;

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            control::{
                choice_from_output::model_choice_from_provider_output,
                choice_repair::repair_model_choice_if_needed,
                finish::{finish_invalid_model_choice, synthesize_loop_answer},
                loop_setup::initialize_primitive_loop,
                model_text_round::{
                    handle_model_text_round, ModelTextRoundInput, ModelTextRoundOutcome,
                },
                native_tool_round::{handle_native_tool_output, NativeToolRoundOutcome},
                provider_error::{handle_provider_loop_error, ProviderLoopErrorOutcome},
                structured_choice_round::{
                    handle_structured_request_choice, handle_structured_requests_choice,
                },
            },
            provider::decision::request_native_tool_loop_response,
            state::{
                logging::{log_loop_model_choice, log_loop_round_started},
                types::PrimitiveHarnessLoopResult,
            },
        },
        ModelChoice, ModelChoiceTurnError,
    },
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

pub fn run_primitive_harness_loop_with_cancel_and_stream<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    cancel: &ProviderCancelToken,
    stream_events: &mut dyn FnMut(Event),
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let mut state = initialize_primitive_loop(session, input);

    loop {
        let round_started = Instant::now();
        log_loop_round_started(session, state.round_index, state.evidence.len());

        let output = match request_native_tool_loop_response(
            provider,
            session,
            &state.messages,
            &state.registry,
            state.round_index,
            cancel,
            stream_events,
        ) {
            Ok(output) => output,
            Err(error) => {
                match handle_provider_loop_error(
                    provider,
                    session,
                    input,
                    error,
                    state.round_index,
                    round_started,
                    &state.budget,
                    &mut state.budget_state,
                    &state.evidence,
                    &mut state.messages,
                    state.rounds,
                    state.loop_turn_id,
                    state.loop_started,
                    cancel,
                    stream_events,
                )? {
                    ProviderLoopErrorOutcome::Retry { returned_rounds } => {
                        state.rounds = returned_rounds;
                        state.round_index = state.round_index.saturating_add(1);
                        continue;
                    }
                    ProviderLoopErrorOutcome::Finish(result) => return Ok(result),
                }
            }
        };

        if !output.tool_calls.is_empty() {
            match handle_native_tool_output(
                provider,
                session,
                input,
                &output,
                state.round_index,
                round_started,
                &state.registry,
                &state.budget,
                &mut state.budget_state,
                &mut state.memory,
                &mut state.rounds,
                &mut state.evidence,
                &mut state.messages,
                state.loop_turn_id,
                state.loop_started,
                cancel,
                stream_events,
            )? {
                NativeToolRoundOutcome::Continue => {}
                NativeToolRoundOutcome::Finish(result) => return Ok(result),
            }
            state.round_index = state.round_index.saturating_add(1);
            continue;
        }

        state.budget_state.decision_calls += 1;
        let mut choice = model_choice_from_provider_output(&output, &state.registry);
        if !state.evidence.is_empty() {
            if let ModelChoice::InvalidStructuredRequest { raw, .. } = &choice {
                choice = ModelChoice::Message {
                    content: raw.clone(),
                };
            }
        }

        log_loop_model_choice(
            session,
            state.round_index,
            round_started.elapsed().as_millis() as u64,
            &choice,
            &output.metrics,
            output
                .metrics
                .as_ref()
                .map(|metrics| metrics.request_id.as_str())
                .unwrap_or("unknown"),
        );

        choice = repair_model_choice_if_needed(
            provider,
            session,
            input,
            &state.registry,
            &state.evidence,
            &state.memory,
            state.round_index,
            &state.budget,
            &mut state.budget_state,
            choice,
            cancel,
        )?;

        match choice {
            ModelChoice::Message { content } => {
                match handle_model_text_round(
                    session,
                    ModelTextRoundInput {
                        input,
                        content,
                        final_stop_reason: if state.evidence.is_empty() {
                            "model_message"
                        } else {
                            "native_final_text"
                        },
                        evidence: &state.evidence,
                        round_index: state.round_index,
                        round_started,
                        provider_claim_retries: &mut state.provider_claim_retries,
                        messages: &mut state.messages,
                        rounds: &mut state.rounds,
                        loop_turn_id: state.loop_turn_id,
                        loop_started: state.loop_started,
                    },
                )? {
                    ModelTextRoundOutcome::Retry => {
                        state.round_index = state.round_index.saturating_add(1);
                        continue;
                    }
                    ModelTextRoundOutcome::Finish(result) => return Ok(result),
                }
            }
            ModelChoice::AnswerNow { evidence_depth, .. } => {
                return synthesize_loop_answer(
                    provider,
                    session,
                    input,
                    &state.evidence,
                    state.rounds,
                    "answer_now".to_string(),
                    evidence_depth,
                    state.loop_turn_id,
                    state.loop_started,
                    cancel,
                    stream_events,
                );
            }
            ModelChoice::StructuredRequest(request) => {
                if let NativeToolRoundOutcome::Finish(result) = handle_structured_request_choice(
                    provider,
                    session,
                    input,
                    request,
                    state.round_index,
                    round_started,
                    &state.registry,
                    &state.budget,
                    &mut state.budget_state,
                    &mut state.memory,
                    &mut state.rounds,
                    &mut state.evidence,
                    &mut state.messages,
                    state.loop_turn_id,
                    state.loop_started,
                    cancel,
                    stream_events,
                )? {
                    return Ok(result);
                }
            }
            ModelChoice::StructuredRequests(requests) => {
                if let NativeToolRoundOutcome::Finish(result) = handle_structured_requests_choice(
                    provider,
                    session,
                    input,
                    requests,
                    state.round_index,
                    round_started,
                    &state.registry,
                    &state.budget,
                    &mut state.budget_state,
                    &mut state.memory,
                    &mut state.rounds,
                    &mut state.evidence,
                    &mut state.messages,
                    state.loop_turn_id,
                    state.loop_started,
                    cancel,
                    stream_events,
                )? {
                    return Ok(result);
                }
            }
            ModelChoice::InvalidStructuredRequest { error, raw } => {
                if !state.evidence.is_empty() {
                    match handle_model_text_round(
                        session,
                        ModelTextRoundInput {
                            input,
                            content: raw,
                            final_stop_reason: "native_final_text",
                            evidence: &state.evidence,
                            round_index: state.round_index,
                            round_started,
                            provider_claim_retries: &mut state.provider_claim_retries,
                            messages: &mut state.messages,
                            rounds: &mut state.rounds,
                            loop_turn_id: state.loop_turn_id,
                            loop_started: state.loop_started,
                        },
                    )? {
                        ModelTextRoundOutcome::Retry => {
                            state.round_index = state.round_index.saturating_add(1);
                            continue;
                        }
                        ModelTextRoundOutcome::Finish(result) => return Ok(result),
                    }
                }

                return finish_invalid_model_choice(
                    session,
                    error.as_str(),
                    state.rounds,
                    state.loop_turn_id,
                    state.loop_started,
                );
            }
        }

        state.round_index = state.round_index.saturating_add(1);
    }
}
