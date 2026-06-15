//! Native tool-call round handling for the primitive harness loop.
//!
//! This module owns one branch of the loop: provider-native tool calls. It
//! parses tool calls, appends tool-call messages, executes validated primitive
//! requests, and reports whether the loop should continue or finish.

use std::time::Instant;

use crate::{
    event::ProviderOutput,
    harness::{
        decide_primitive_permission,
        harness_loop::{
            control::{
                choice_from_output::native_tool_requests_from_provider_output,
                choice_from_output::NativeToolRequest, finish::synthesize_loop_answer,
                native_execution::execute_native_tool_request,
            },
            evidence::render::permission_evidence,
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_harness_approval_requested, log_harness_batch_approval_requested,
                    log_loop_evidence, log_loop_round_finished, log_permission_decision,
                },
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
            },
        },
        EvidenceDepth, ModelChoiceTurnError, PendingApproval, PermissionDecisionKind,
        PrimitiveToolRegistry, StructuredRequestKind, ValidatedStructuredRequest,
    },
    provider::{ChatMessage, ControllerProvider, ProviderCancelToken},
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
    cancel: &ProviderCancelToken,
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
                cancel,
            )?;
            return Ok(NativeToolRoundOutcome::Finish(result));
        }
    };

    messages.push(
        ChatMessage::assistant(output.text.clone()).with_tool_calls(output.tool_calls.clone()),
    );

    if risky_request_count(session, &native_requests) > 1 {
        return handle_native_risky_batch(
            provider,
            session,
            input,
            native_requests,
            round_index,
            round_started,
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
        );
    }

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
            cancel,
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

#[allow(clippy::too_many_arguments)]
fn handle_native_risky_batch<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    native_requests: Vec<NativeToolRequest>,
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
) -> Result<NativeToolRoundOutcome, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let risky_requests = native_requests
        .iter()
        .filter(|native_request| is_risky_request(session, &native_request.request))
        .map(|native_request| native_request.request.clone())
        .collect::<Vec<_>>();
    let approval_id = session.next_approval_id();
    let approval = PendingApproval::from_requests_with_launch_cwd(
        &approval_id,
        &risky_requests,
        "multiple risky primitives require approval before side-effect execution",
        &session.cwd,
    )
    .expect("risky batch has at least one request");
    session.set_pending_approval(approval.clone());
    log_harness_approval_requested(session, round_index, &approval);
    log_harness_batch_approval_requested(session, round_index, &approval);

    let mut batch_step = 0usize;
    for native_request in native_requests {
        if is_risky_request(session, &native_request.request) {
            batch_step = batch_step.saturating_add(1);
            collect_batch_permission_evidence(
                session,
                &approval,
                &native_request,
                round_index,
                batch_step,
                risky_requests.len(),
                rounds,
                evidence,
                messages,
            );
            continue;
        }

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
            cancel,
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

#[allow(clippy::too_many_arguments)]
fn collect_batch_permission_evidence(
    session: &Session,
    approval: &PendingApproval,
    native_request: &NativeToolRequest,
    round_index: usize,
    batch_step: usize,
    batch_step_count: usize,
    rounds: &mut Vec<PrimitiveHarnessLoopRound>,
    evidence: &mut Vec<Evidence>,
    messages: &mut Vec<ChatMessage>,
) {
    let permission = decide_primitive_permission(
        &PrimitiveToolRegistry::stage_3a(),
        &native_request.request,
        session.permission_mode(),
    );
    log_permission_decision(session, round_index, &native_request.request, &permission);
    let mut evidence_item = permission_evidence(
        format!(
            "batch:{}:{}:{}",
            approval.id,
            batch_step,
            native_request.request.kind.as_str()
        ),
        &native_request.request,
        &permission,
        Some(&approval.id),
    );
    evidence_item.body.push_str(&format!(
        "batch_step: {batch_step}\nbatch_step_count: {batch_step_count}\n"
    ));
    log_loop_evidence(session, round_index, &evidence_item);
    rounds.push(PrimitiveHarnessLoopRound {
        round_index,
        tool: Some(native_request.request.kind.as_str().to_string()),
        evidence_label: Some(evidence_item.label.clone()),
    });
    messages.push(ChatMessage::tool(
        native_request.tool_call_id.clone(),
        evidence_item.body.clone(),
    ));
    evidence.push(evidence_item);
}

fn risky_request_count(session: &Session, native_requests: &[NativeToolRequest]) -> usize {
    native_requests
        .iter()
        .filter(|native_request| is_risky_request(session, &native_request.request))
        .count()
}

fn is_risky_request(session: &Session, request: &ValidatedStructuredRequest) -> bool {
    matches!(
        request.kind,
        StructuredRequestKind::Bash | StructuredRequestKind::Write | StructuredRequestKind::Edit
    ) && matches!(
        decide_primitive_permission(
            &PrimitiveToolRegistry::stage_3a(),
            request,
            session.permission_mode()
        )
        .kind,
        PermissionDecisionKind::NeedsApproval
    )
}
