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
                    NativeToolRequest,
                },
                finish::{
                    finish_invalid_model_choice, finish_with_model_message, synthesize_loop_answer,
                },
                request_handling::{collect_request_evidence, RequestHandlingOutcome},
                start::log_loop_started,
            },
            provider::repair::request_model_choice_repair,
            provider::{
                context::native_tool_loop_initial_messages,
                decision::request_native_tool_loop_response,
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_loop_model_choice, log_loop_repair_finished, log_loop_repair_started,
                    log_loop_round_finished, log_loop_round_started,
                },
                memory::HarnessWorkingMemory,
                types::PrimitiveHarnessLoopResult,
            },
        },
        EvidenceDepth, ModelChoice, ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ChatToolCall, ChatToolCallFunction, ControllerProvider},
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
    let mut messages = native_tool_loop_initial_messages(input);

    log_loop_started(session, loop_turn_id, input, &budget);

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

#[allow(clippy::too_many_arguments)]
fn execute_native_tool_request<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    native_request: NativeToolRequest,
    round_index: usize,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    memory: &mut HarnessWorkingMemory,
    rounds: &mut Vec<crate::harness::PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<crate::harness::harness_loop::state::types::Evidence>,
    messages: &mut Vec<ChatMessage>,
    loop_turn_id: u64,
    loop_started: Instant,
) -> Result<Option<PrimitiveHarnessLoopResult>, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let evidence_before = evidence.len();
    match collect_request_evidence(
        session,
        &native_request.request,
        round_index,
        budget,
        budget_state,
        memory,
        rounds,
        evidence,
    ) {
        Ok(RequestHandlingOutcome::UsefulEvidence | RequestHandlingOutcome::ExecutionFailed) => {
            let body = evidence
                .get(evidence_before)
                .map(|item| item.body.clone())
                .unwrap_or_else(|| {
                    "VERIFIED_LOOP_NOTICE\nNo new evidence was collected for this request."
                        .to_string()
                });
            messages.push(ChatMessage::tool(native_request.tool_call_id, body));
            Ok(None)
        }
        Ok(RequestHandlingOutcome::NoProgress) => {
            messages.push(ChatMessage::tool(
                native_request.tool_call_id,
                "VERIFIED_LOOP_NOTICE\nDuplicate or no-op request rejected. Use the existing verified evidence or choose a different tool request.",
            ));
            Ok(None)
        }
        Err(stop_reason) => synthesize_loop_answer(
            provider,
            session,
            input,
            evidence,
            std::mem::take(rounds),
            stop_reason,
            EvidenceDepth::Limited,
            loop_turn_id,
            loop_started,
        )
        .map(Some),
    }
}

fn synthetic_native_tool_request(
    round_index: usize,
    request_index: usize,
    request: crate::harness::ValidatedStructuredRequest,
) -> NativeToolRequest {
    NativeToolRequest {
        tool_call_id: format!("json-fallback-{round_index}-{request_index}"),
        request,
    }
}

fn synthetic_assistant_tool_call(request: &NativeToolRequest) -> ChatMessage {
    synthetic_assistant_tool_calls(std::slice::from_ref(request))
}

fn synthetic_assistant_tool_calls(requests: &[NativeToolRequest]) -> ChatMessage {
    let tool_calls = requests
        .iter()
        .map(|request| ChatToolCall {
            id: request.tool_call_id.clone(),
            tool_type: "function".to_string(),
            function: ChatToolCallFunction {
                name: request.request.kind.as_str().to_string(),
                arguments: request
                    .request
                    .arguments
                    .as_ref()
                    .map(serde_json::Value::to_string)
                    .unwrap_or_else(|| "{}".to_string()),
            },
        })
        .collect::<Vec<_>>();

    ChatMessage::assistant("").with_tool_calls(tool_calls)
}

fn repair_needed_for_choice(choice: &ModelChoice) -> Option<(String, String)> {
    match choice {
        ModelChoice::InvalidStructuredRequest { error, raw } => Some((error.as_str(), raw.clone())),
        _ => None,
    }
}
