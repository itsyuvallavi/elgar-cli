//! Native tool request execution bridge for the primitive harness loop.
//!
//! The coordinator decides loop order; this file handles the repeated mechanics
//! of executing one validated tool request and returning its tool-result
//! message to the provider conversation.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::{
                choice_from_output::NativeToolRequest,
                finish::synthesize_loop_answer,
                request_handling::{collect_request_evidence, RequestHandlingOutcome},
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopResult},
            },
        },
        EvidenceDepth, ModelChoiceTurnError, PrimitiveHarnessLoopRound, PrimitiveToolRegistry,
    },
    provider::{ChatMessage, ControllerProvider},
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
) -> Result<Option<PrimitiveHarnessLoopResult>, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
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
