//! Retry feedback for rejected provider claims.
//!
//! This module does not inspect user prompts or infer intent. It only renders
//! runtime feedback after provider prose was rejected by a guard.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::{
                finish::finish_with_model_message,
                prose_claim_guard::{validate_provider_final_text, ProseClaimGuardDecision},
                tool_target_fidelity::explicit_primitive_request,
            },
            state::{
                logging::log_loop_round_finished,
                types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
            },
        },
        ModelChoiceTurnError,
    },
    provider::ChatMessage,
    session::Session,
};

pub(super) const MAX_PROVIDER_CLAIM_RETRIES: usize = 1;

pub(super) enum ProviderClaimGuardOutcome {
    Allow,
    Retried,
    Block {
        reason: &'static str,
        final_text: &'static str,
    },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn guard_provider_text_or_retry(
    session: &Session,
    input: &str,
    content: &str,
    evidence: &[Evidence],
    round_index: usize,
    round_started: Instant,
    retry_count: &mut usize,
    messages: &mut Vec<ChatMessage>,
) -> ProviderClaimGuardOutcome {
    if let ProseClaimGuardDecision::Block { reason } =
        validate_provider_final_text(session, content, evidence)
    {
        return retry_or_block(
            session,
            round_index,
            round_started,
            retry_count,
            messages,
            "Your previous answer claimed local project actions or local file facts, but this turn has no verified tool evidence. Request a primitive tool if local evidence is needed, or answer without local project/file claims.",
            "unverified_claim_retry",
            reason,
            "I need verified tool evidence before claiming local project actions or file facts.",
        );
    }

    if evidence.is_empty() {
        if let Some(request) = explicit_primitive_request(input) {
            return retry_or_block(
                session,
                round_index,
                round_started,
                retry_count,
                messages,
                request.missing_evidence_feedback(),
                "missing_primitive_evidence_retry",
                "missing_primitive_evidence",
                "Direct primitive requests need verified tool evidence before answering.",
            );
        }
    }

    ProviderClaimGuardOutcome::Allow
}

#[allow(clippy::too_many_arguments)]
fn retry_or_block(
    session: &Session,
    round_index: usize,
    round_started: Instant,
    retry_count: &mut usize,
    messages: &mut Vec<ChatMessage>,
    feedback: impl AsRef<str>,
    log_reason: &'static str,
    reason: &'static str,
    final_text: &'static str,
) -> ProviderClaimGuardOutcome {
    if *retry_count >= MAX_PROVIDER_CLAIM_RETRIES {
        return ProviderClaimGuardOutcome::Block { reason, final_text };
    }

    *retry_count = retry_count.saturating_add(1);
    messages.push(ChatMessage::system(format!(
        "RUNTIME VALIDATION: {}",
        feedback.as_ref()
    )));
    log_loop_round_finished(session, round_index, round_started, log_reason);
    ProviderClaimGuardOutcome::Retried
}

pub(super) fn finish_provider_claim_block(
    session: &Session,
    rounds: Vec<PrimitiveHarnessLoopRound>,
    reason: &str,
    final_text: &'static str,
    loop_turn_id: u64,
    loop_started: Instant,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError> {
    finish_with_model_message(
        session,
        final_text.to_string(),
        rounds,
        reason.to_string(),
        loop_turn_id,
        loop_started,
    )
}
