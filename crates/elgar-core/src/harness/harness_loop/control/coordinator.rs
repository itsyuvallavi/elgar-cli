//! Coordinator for the primitive harness loop.
//!
//! This file owns loop order only: ask the model what evidence it wants,
//! collect verified evidence, and choose the stop path. Parsing, execution,
//! provider calls, and finishing live in sibling control/provider modules.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::{
                choice_from_output::model_choice_from_provider_output,
                choice_repair::repair_model_choice_if_needed,
                finish::{
                    finish_invalid_model_choice, finish_with_model_message, synthesize_loop_answer,
                },
                native_tool_round::{handle_native_tool_output, NativeToolRoundOutcome},
                provider_claim_retry::{
                    finish_provider_claim_block, guard_provider_text_or_retry,
                    ProviderClaimGuardOutcome,
                },
                start::log_loop_started,
                structured_choice_round::{
                    handle_structured_request_choice, handle_structured_requests_choice,
                },
            },
            provider::{
                context::native_tool_loop_initial_messages,
                decision::request_native_tool_loop_response,
                session_context::TurnPromptContextStats,
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{log_loop_model_choice, log_loop_round_started, log_turn_prompt_context},
                memory::HarnessWorkingMemory,
                types::PrimitiveHarnessLoopResult,
            },
        },
        ModelChoice, ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    mcp::config::load_runtime_mcp_config,
    provider::ControllerProvider,
    session::Session,
};

/// Run a primitive loop for one harness turn.
pub fn run_primitive_harness_loop<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let loop_turn_id = session.next_turn_id();
    let loop_started = Instant::now();
    let registry =
        PrimitiveToolRegistry::stage_3a_with_mcp(mcp_config_is_available(&session.project_root));
    let budget = PrimitiveLoopBudget::default();
    let mut budget_state = PrimitiveLoopBudgetState::default();
    let mut rounds = Vec::new();
    let mut evidence = Vec::new();
    let mut memory = HarnessWorkingMemory::default();
    let mut provider_claim_retries = 0usize;
    let turn_context = native_tool_loop_initial_messages(session, input);
    let TurnPromptContextStats {
        initial_message_count,
        history_turns,
        memory: memory_stats,
        ..
    } = turn_context.stats;
    let mut messages = turn_context.messages;

    log_loop_started(session, loop_turn_id, input, &budget);
    log_turn_prompt_context(session, initial_message_count, history_turns, &memory_stats);

    let mut round_index = 0usize;
    loop {
        let round_started = Instant::now();
        log_loop_round_started(session, round_index, evidence.len());

        let output = request_native_tool_loop_response(
            provider,
            session,
            &messages,
            &registry,
            round_index,
        )?;

        if !output.tool_calls.is_empty() {
            match handle_native_tool_output(
                provider,
                session,
                input,
                &output,
                round_index,
                round_started,
                &registry,
                &budget,
                &mut budget_state,
                &mut memory,
                &mut rounds,
                &mut evidence,
                &mut messages,
                loop_turn_id,
                loop_started,
            )? {
                NativeToolRoundOutcome::Continue => {}
                NativeToolRoundOutcome::Finish(result) => return Ok(result),
            }
            round_index = round_index.saturating_add(1);
            continue;
        }

        budget_state.decision_calls += 1;
        let mut choice = model_choice_from_provider_output(&output, &registry);
        if !evidence.is_empty() {
            if let ModelChoice::InvalidStructuredRequest { raw, .. } = &choice {
                choice = ModelChoice::Message {
                    content: raw.clone(),
                };
            }
        }

        log_loop_model_choice(
            session,
            round_index,
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
            &registry,
            &evidence,
            &memory,
            round_index,
            &budget,
            &mut budget_state,
            choice,
        )?;

        match choice {
            ModelChoice::Message { content } => {
                match guard_provider_text_or_retry(
                    session,
                    input,
                    &content,
                    &evidence,
                    round_index,
                    round_started,
                    &mut provider_claim_retries,
                    &mut messages,
                ) {
                    ProviderClaimGuardOutcome::Allow => {}
                    ProviderClaimGuardOutcome::Retried => {
                        round_index = round_index.saturating_add(1);
                        continue;
                    }
                    ProviderClaimGuardOutcome::Block { reason, final_text } => {
                        return finish_provider_claim_block(
                            session,
                            rounds,
                            reason,
                            final_text,
                            loop_turn_id,
                            loop_started,
                        );
                    }
                }
                return finish_with_model_message(
                    session,
                    content,
                    rounds,
                    if evidence.is_empty() {
                        "model_message".to_string()
                    } else {
                        "native_final_text".to_string()
                    },
                    loop_turn_id,
                    loop_started,
                );
            }
            ModelChoice::AnswerNow { evidence_depth, .. } => {
                return synthesize_loop_answer(
                    provider,
                    session,
                    input,
                    &evidence,
                    rounds,
                    "answer_now".to_string(),
                    evidence_depth,
                    loop_turn_id,
                    loop_started,
                );
            }
            ModelChoice::StructuredRequest(request) => {
                if let NativeToolRoundOutcome::Finish(result) = handle_structured_request_choice(
                    provider,
                    session,
                    input,
                    request,
                    round_index,
                    round_started,
                    &registry,
                    &budget,
                    &mut budget_state,
                    &mut memory,
                    &mut rounds,
                    &mut evidence,
                    &mut messages,
                    loop_turn_id,
                    loop_started,
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
                    round_index,
                    round_started,
                    &registry,
                    &budget,
                    &mut budget_state,
                    &mut memory,
                    &mut rounds,
                    &mut evidence,
                    &mut messages,
                    loop_turn_id,
                    loop_started,
                )? {
                    return Ok(result);
                }
            }
            ModelChoice::InvalidStructuredRequest { error, raw } => {
                if !evidence.is_empty() {
                    match guard_provider_text_or_retry(
                        session,
                        input,
                        &raw,
                        &evidence,
                        round_index,
                        round_started,
                        &mut provider_claim_retries,
                        &mut messages,
                    ) {
                        ProviderClaimGuardOutcome::Allow => {}
                        ProviderClaimGuardOutcome::Retried => {
                            round_index = round_index.saturating_add(1);
                            continue;
                        }
                        ProviderClaimGuardOutcome::Block { reason, final_text } => {
                            return finish_provider_claim_block(
                                session,
                                rounds,
                                reason,
                                final_text,
                                loop_turn_id,
                                loop_started,
                            );
                        }
                    }
                    return finish_with_model_message(
                        session,
                        raw,
                        rounds,
                        "native_final_text".to_string(),
                        loop_turn_id,
                        loop_started,
                    );
                }

                return finish_invalid_model_choice(
                    session,
                    error.as_str(),
                    rounds,
                    loop_turn_id,
                    loop_started,
                );
            }
        }

        round_index = round_index.saturating_add(1);
    }
}

fn mcp_config_is_available(project_root: &std::path::Path) -> bool {
    load_runtime_mcp_config(project_root)
        .ok()
        .flatten()
        .is_some()
}
