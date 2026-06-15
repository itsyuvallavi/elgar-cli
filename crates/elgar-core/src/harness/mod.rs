//! Harness entry point for one Elgar model turn.
//!
//! The harness is now the single route from CLI/TUI input to the model. It asks
//! the model which primitive evidence it wants, validates and executes the
//! currently enabled primitives locally, then records one visible assistant
//! answer.

mod context;
mod harness_loop;
mod memory;
mod model_choice;
mod permissions;
mod primitive_tools;
mod provider_route;
mod tool_definitions;

#[cfg(test)]
mod tests;

use std::time::Instant;

use serde_json::json;

use crate::{
    event::{AssistantMessage, AssistantMessageSource, ErrorEvent, Event, UserMessage},
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

pub use context::{
    collect_directory_summary, collect_find_matches, collect_grep_matches, collect_project_file,
    DirectoryEntry, DirectoryEntryKind, DirectoryError, DirectoryOmission, DirectoryOptions,
    DirectorySnapshot, FindError, FindOptions, FindSnapshot, GrepError, GrepMatch, GrepOptions,
    GrepSnapshot, ProjectFileError, ProjectFileOptions, ProjectFileSnapshot,
};
pub use harness_loop::{
    render_primitive_harness_loop_result, run_primitive_harness_loop,
    run_primitive_harness_loop_with_cancel, PrimitiveHarnessLoopResult, PrimitiveHarnessLoopRound,
};
pub use memory::{
    build_memory_index, read_session_memory_events, render_verified_memory_for_prompt_with_budget,
    HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind, HarnessMemoryPromptBudget,
    RenderedMemoryPrompt, RenderedMemoryStats, SessionMemoryReadError,
};
pub use model_choice::{
    loop_decision_contract, model_choice_contract, parse_model_choice,
    parse_model_choice_with_registry, EvidenceDepth, ModelChoice, ModelChoiceTurnError,
    StructuredRequestKind, StructuredRequestValidationError, ValidatedStructuredRequest,
    MAX_TOOL_CALL_BATCH,
};
pub(in crate::harness) use permissions::resolve_write_target;
pub use permissions::{
    approve_pending_approval, decide_primitive_permission, deny_pending_approval,
    ApprovalCommandError, ApprovalCommandResult, ApprovalTargetPreview, ApprovalTargetScope,
    PendingApproval, PendingApprovalStatus, PendingApprovalStep, PermissionDecision,
    PermissionDecisionKind, PermissionMode,
};
pub use primitive_tools::{
    PrimitiveTool, PrimitiveToolId, PrimitiveToolRegistry, PrimitiveToolSideEffectLevel,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessTurnResult {
    pub events: Vec<Event>,
}

/// Runs one harness-controlled model turn.
///
/// This is the normal CLI/TUI path. There is no direct model bypass here:
/// model calls go through the primitive harness loop and verified evidence.
pub fn run_harness_turn<P>(provider: &P, session: &mut Session, input: &str) -> HarnessTurnResult
where
    P: ControllerProvider,
{
    run_harness_turn_with_cancel(provider, session, input, &ProviderCancelToken::new())
}

/// Runs one harness-controlled model turn with a cooperative cancellation token.
pub fn run_harness_turn_with_cancel<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    cancel: &ProviderCancelToken,
) -> HarnessTurnResult
where
    P: ControllerProvider,
{
    let start_index = session.events().len();
    let turn_id = session.next_turn_id();
    let started = Instant::now();
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_harness_turn",
            "harness_turn_started",
        )
        .with_metadata(json!({
            "input_chars": input.chars().count(),
            "harness_mode": "read_only_primitive_loop"
        })),
    );

    session.push_event(Event::UserMessage(UserMessage::new(input)));

    let loop_result = run_primitive_harness_loop_with_cancel(provider, session, input, cancel);
    match loop_result {
        Ok(result) => {
            let final_text = result
                .final_text
                .unwrap_or_else(|| "No final model message.".to_string());
            session.push_event(Event::AssistantMessage(AssistantMessage::new(
                final_text,
                AssistantMessageSource::Provider,
            )));
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "harness turn failed: {error}"
            ))));
        }
    }

    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Runtime,
            file!(),
            "run_harness_turn",
            "harness_turn_finished",
        )
        .with_duration_ms(started.elapsed().as_millis() as u64)
        .with_metadata(json!({
            "events_created": session.events().len().saturating_sub(start_index),
            "harness_mode": "read_only_primitive_loop"
        })),
    );

    HarnessTurnResult {
        events: session.events()[start_index..].to_vec(),
    }
}
