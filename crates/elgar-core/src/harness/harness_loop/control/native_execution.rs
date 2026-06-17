//! Native tool request execution bridge for the primitive harness loop.
//!
//! The coordinator decides loop order; this file handles the repeated mechanics
//! of executing one validated tool request and returning its tool-result
//! message to the provider conversation.

use std::time::Instant;

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            control::{
                choice_from_output::NativeToolRequest,
                finish::{finish_with_model_message, synthesize_loop_answer},
                request_handling::{collect_request_evidence, RequestHandlingOutcome},
                tool_target_fidelity::validate_tool_target,
            },
            evidence::timeline::append_verified_action_timeline,
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::log_verified_action_timeline_appended,
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopResult},
            },
        },
        EvidenceDepth, ModelChoiceTurnError, PrimitiveHarnessLoopRound, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ControllerProvider, ProviderCancelToken},
    session::Session,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn execute_native_tool_request<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    native_request: NativeToolRequest,
    round_index: usize,
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
) -> Result<Option<PrimitiveHarnessLoopResult>, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    if let Some(mismatch) = validate_tool_target(input, &native_request.request) {
        budget_state.target_mismatches = budget_state.target_mismatches.saturating_add(1);
        rounds.push(PrimitiveHarnessLoopRound {
            round_index,
            tool: Some("notice".to_string()),
            evidence_label: Some(mismatch.reason.to_string()),
        });
        if budget_state.target_mismatches > budget.max_target_mismatches {
            return synthesize_loop_answer(
                provider,
                session,
                input,
                evidence,
                std::mem::take(rounds),
                mismatch.reason.to_string(),
                EvidenceDepth::Limited,
                loop_turn_id,
                loop_started,
                cancel,
                stream_events,
            )
            .map(Some);
        }
        messages.push(ChatMessage::tool(
            native_request.tool_call_id,
            mismatch.notice,
        ));
        return Ok(None);
    }

    let evidence_before = evidence.len();
    match collect_request_evidence(
        session,
        registry,
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
            if let Some(stats) =
                crate::harness::harness_loop::evidence::timeline::verified_action_timeline_stats(
                    evidence,
                )
            {
                log_verified_action_timeline_appended(session, round_index, stats);
            }
            messages.push(ChatMessage::tool(
                native_request.tool_call_id,
                append_verified_action_timeline(&body, evidence),
            ));
            Ok(None)
        }
        Ok(RequestHandlingOutcome::PendingApproval) => {
            let body = evidence
                .get(evidence_before)
                .map(|item| item.body.clone())
                .unwrap_or_else(|| {
                    "VERIFIED_PERMISSION_DECISION\napproval_required: true\nexecution_performed: false\n"
                        .to_string()
                });
            messages.push(ChatMessage::tool(native_request.tool_call_id, body));
            finish_with_model_message(
                session,
                pending_approval_message(session),
                std::mem::take(rounds),
                "approval_pending".to_string(),
                loop_turn_id,
                loop_started,
            )
            .map(Some)
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
            cancel,
            stream_events,
        )
        .map(Some),
    }
}

fn pending_approval_message(session: &Session) -> String {
    let Some(approval) = session.pending_approval() else {
        return "Requested action is prepared and waiting for approval before execution."
            .to_string();
    };
    if approval.is_batch() {
        return format!(
            "{} requested actions are prepared and waiting for approval before execution.",
            approval.steps.len()
        );
    }
    match approval.target_preview.as_ref() {
        Some(target) => format!(
            "`{}` on `{}` is prepared and waiting for approval before execution.",
            approval.tool, target.requested_path
        ),
        None => format!(
            "`{}` is prepared and waiting for approval before execution.",
            approval.tool
        ),
    }
}
