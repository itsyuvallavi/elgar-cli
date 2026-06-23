//! Model-choice repair handling for the primitive harness loop.
//!
//! Repair is only attempted before evidence exists. Once evidence exists,
//! provider prose can be a valid final answer over tool results.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::choice_from_output::model_choice_from_provider_output,
            provider::repair::request_model_choice_repair,
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                logging::{
                    log_loop_model_choice, log_loop_repair_finished, log_loop_repair_started,
                },
                memory::HarnessWorkingMemory,
                types::Evidence,
            },
        },
        ModelChoice, ModelChoiceTurnError, PrimitiveToolRegistry,
    },
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

#[allow(clippy::too_many_arguments)]
pub(super) fn repair_model_choice_if_needed<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    registry: &PrimitiveToolRegistry,
    evidence: &[Evidence],
    memory: &HarnessWorkingMemory,
    round_index: usize,
    budget: &PrimitiveLoopBudget,
    budget_state: &mut PrimitiveLoopBudgetState,
    choice: ModelChoice,
    cancel: &ProviderCancelToken,
) -> Result<ModelChoice, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    if !evidence.is_empty() {
        return Ok(choice);
    }

    let Some((error_text, raw_text)) = repair_needed_for_choice(&choice) else {
        return Ok(choice);
    };

    if budget_state.repair_attempts >= budget.max_repair_attempts {
        return Ok(choice);
    }

    let repair_started = Instant::now();
    log_loop_repair_started(session, round_index, &error_text, &raw_text);
    let repair_output = request_model_choice_repair(
        provider,
        session,
        input,
        registry,
        evidence,
        memory,
        round_index,
        &error_text,
        &raw_text,
        cancel,
    )?;
    budget_state.repair_attempts += 1;
    let repaired_choice = model_choice_from_provider_output(&repair_output, registry);
    log_loop_model_choice(
        session,
        round_index,
        repair_started.elapsed().as_millis() as u64,
        &repaired_choice,
        &repair_output.metrics,
        repair_output
            .metrics
            .as_ref()
            .map(|metrics| metrics.request_id.as_str())
            .unwrap_or("unknown"),
    );
    log_loop_repair_finished(session, round_index, repair_started, &repaired_choice);

    Ok(repaired_choice)
}

fn repair_needed_for_choice(choice: &ModelChoice) -> Option<(String, String)> {
    match choice {
        ModelChoice::InvalidStructuredRequest { error, raw } => Some((error.as_str(), raw.clone())),
        _ => None,
    }
}
