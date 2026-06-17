//! Text-finalization branch for the primitive harness loop.
//!
//! This module owns provider prose claim checks after a provider response has no
//! tool calls. It does not request providers or execute tools.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::{
                finish::finish_with_model_message,
                provider_claim_retry::{
                    finish_provider_claim_block, guard_provider_text_or_retry,
                    ProviderClaimGuardOutcome,
                },
            },
            state::types::{Evidence, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound},
        },
        ModelChoiceTurnError,
    },
    provider::ChatMessage,
    session::Session,
};

pub(crate) enum ModelTextRoundOutcome {
    Retry,
    Finish(PrimitiveHarnessLoopResult),
}

pub(crate) struct ModelTextRoundInput<'a> {
    pub(crate) input: &'a str,
    pub(crate) content: String,
    pub(crate) final_stop_reason: &'a str,
    pub(crate) evidence: &'a [Evidence],
    pub(crate) round_index: usize,
    pub(crate) round_started: Instant,
    pub(crate) provider_claim_retries: &'a mut usize,
    pub(crate) messages: &'a mut Vec<ChatMessage>,
    pub(crate) rounds: &'a mut Vec<PrimitiveHarnessLoopRound>,
    pub(crate) loop_turn_id: u64,
    pub(crate) loop_started: Instant,
}

pub(crate) fn handle_model_text_round(
    session: &mut Session,
    args: ModelTextRoundInput<'_>,
) -> Result<ModelTextRoundOutcome, ModelChoiceTurnError> {
    match guard_provider_text_or_retry(
        session,
        args.input,
        &args.content,
        args.evidence,
        args.round_index,
        args.round_started,
        args.provider_claim_retries,
        args.messages,
    ) {
        ProviderClaimGuardOutcome::Allow => {}
        ProviderClaimGuardOutcome::Retried => return Ok(ModelTextRoundOutcome::Retry),
        ProviderClaimGuardOutcome::Block { reason, final_text } => {
            return finish_provider_claim_block(
                session,
                std::mem::take(args.rounds),
                reason,
                final_text,
                args.loop_turn_id,
                args.loop_started,
            )
            .map(ModelTextRoundOutcome::Finish);
        }
    }

    finish_with_model_message(
        session,
        args.content,
        std::mem::take(args.rounds),
        args.final_stop_reason.to_string(),
        args.loop_turn_id,
        args.loop_started,
    )
    .map(ModelTextRoundOutcome::Finish)
}
