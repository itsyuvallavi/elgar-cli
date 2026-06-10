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
                choice_from_output::{
                    model_choice_from_provider_output, native_tool_requests_from_provider_output,
                },
                finish::{
                    finish_invalid_model_choice, finish_with_model_message, synthesize_loop_answer,
                },
                native_execution::execute_native_tool_request,
                start::log_loop_started,
                synthetic_tool_calls::{
                    synthetic_assistant_tool_call, synthetic_assistant_tool_calls,
                    synthetic_native_tool_request,
                },
            },
            provider::repair::request_model_choice_repair,
            provider::{
                context::native_tool_loop_initial_messages,
                decision::request_native_tool_loop_response,
                session_context::TurnPromptContextStats,
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_loop_model_choice, log_loop_repair_finished, log_loop_repair_started,
                    log_loop_round_finished, log_loop_round_started, log_turn_prompt_context,
                },
                memory::HarnessWorkingMemory,
                types::PrimitiveHarnessLoopResult,
            },
        },
        EvidenceDepth, ModelChoice, ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ControllerProvider},
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
    let registry = PrimitiveToolRegistry::stage_3a();
    let budget = PrimitiveLoopBudget::default();
    let mut budget_state = PrimitiveLoopBudgetState::default();
    let mut rounds = Vec::new();
    let mut evidence = Vec::new();
    let mut memory = HarnessWorkingMemory::default();
    let turn_context = native_tool_loop_initial_messages(session, input);
    let TurnPromptContextStats {
        initial_message_count,
        history_turns,
        verified_fact_count,
    } = turn_context.stats;
    let mut messages = turn_context.messages;

    log_loop_started(session, loop_turn_id, input, &budget);
    log_turn_prompt_context(
        session,
        initial_message_count,
        history_turns,
        verified_fact_count,
    );

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
            let native_requests =
                match native_tool_requests_from_provider_output(&output, &registry) {
                    Ok(requests) => requests,
                    Err(error) => {
                        return synthesize_loop_answer(
                            provider,
                            session,
                            input,
                            &evidence,
                            rounds,
                            error.as_str(),
                            EvidenceDepth::Limited,
                            loop_turn_id,
                            loop_started,
                        );
                    }
                };

            messages.push(
                ChatMessage::assistant(output.text.clone())
                    .with_tool_calls(output.tool_calls.clone()),
            );

            for native_request in native_requests {
                if let Some(result) = execute_native_tool_request(
                    provider,
                    session,
                    input,
                    native_request,
                    round_index,
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

            budget_state.decision_calls += 1;
            log_loop_round_finished(
                session,
                round_index,
                round_started,
                "native_tool_results_collected",
            );
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

        if evidence.is_empty() {
            if let Some((error_text, raw_text)) = repair_needed_for_choice(&choice) {
                if budget_state.repair_attempts < budget.max_repair_attempts {
                    let repair_started = Instant::now();
                    log_loop_repair_started(session, round_index, &error_text, &raw_text);
                    let repair_output = request_model_choice_repair(
                        provider,
                        session,
                        input,
                        &registry,
                        &evidence,
                        &memory,
                        round_index,
                        &error_text,
                        &raw_text,
                    )?;
                    budget_state.repair_attempts += 1;
                    choice = model_choice_from_provider_output(&repair_output, &registry);
                    log_loop_model_choice(
                        session,
                        round_index,
                        repair_started.elapsed().as_millis() as u64,
                        &choice,
                        &repair_output.metrics,
                        repair_output
                            .metrics
                            .as_ref()
                            .map(|metrics| metrics.request_id.as_str())
                            .unwrap_or("unknown"),
                    );
                    log_loop_repair_finished(session, round_index, repair_started, &choice);
                }
            }
        }

        match choice {
            ModelChoice::Message { content } => {
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
                let synthetic = synthetic_native_tool_request(round_index, 0, request);
                messages.push(synthetic_assistant_tool_call(&synthetic));
                if let Some(result) = execute_native_tool_request(
                    provider,
                    session,
                    input,
                    synthetic,
                    round_index,
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
                log_loop_round_finished(session, round_index, round_started, "evidence_collected");
            }
            ModelChoice::StructuredRequests(requests) => {
                let synthetic_requests = requests
                    .into_iter()
                    .enumerate()
                    .map(|(index, request)| {
                        synthetic_native_tool_request(round_index, index, request)
                    })
                    .collect::<Vec<_>>();
                messages.push(synthetic_assistant_tool_calls(&synthetic_requests));
                for request in synthetic_requests {
                    if let Some(result) = execute_native_tool_request(
                        provider,
                        session,
                        input,
                        request,
                        round_index,
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
                log_loop_round_finished(
                    session,
                    round_index,
                    round_started,
                    "batch_evidence_collected",
                );
            }
            ModelChoice::InvalidStructuredRequest { error, raw } => {
                if !evidence.is_empty() {
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

fn repair_needed_for_choice(choice: &ModelChoice) -> Option<(String, String)> {
    match choice {
        ModelChoice::InvalidStructuredRequest { error, raw } => Some((error.as_str(), raw.clone())),
        _ => None,
    }
}
