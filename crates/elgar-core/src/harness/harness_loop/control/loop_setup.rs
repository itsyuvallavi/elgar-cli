//! Setup state for one primitive harness loop.
//!
//! This module keeps coordinator startup mechanical: build the registry,
//! budget, memory, and initial provider messages, then log the turn context.

use std::time::Instant;

use crate::{
    harness::{
        harness_loop::{
            control::start::log_loop_started,
            provider::{
                context::native_tool_loop_initial_messages, session_context::TurnPromptContextStats,
            },
            state::{
                budget::{PrimitiveLoopBudget, PrimitiveLoopBudgetState},
                memory::HarnessWorkingMemory,
                types::{Evidence, PrimitiveHarnessLoopRound},
            },
        },
        PrimitiveToolRegistry,
    },
    mcp::config::load_runtime_mcp_config,
    provider::ChatMessage,
    session::Session,
};

use super::super::state::logging::log_turn_prompt_context;

pub(crate) struct PrimitiveLoopState {
    pub(crate) loop_turn_id: u64,
    pub(crate) loop_started: Instant,
    pub(crate) registry: PrimitiveToolRegistry,
    pub(crate) budget: PrimitiveLoopBudget,
    pub(crate) budget_state: PrimitiveLoopBudgetState,
    pub(crate) rounds: Vec<PrimitiveHarnessLoopRound>,
    pub(crate) evidence: Vec<Evidence>,
    pub(crate) memory: HarnessWorkingMemory,
    pub(crate) provider_claim_retries: usize,
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) round_index: usize,
}

pub(crate) fn initialize_primitive_loop(session: &mut Session, input: &str) -> PrimitiveLoopState {
    let loop_turn_id = session.next_turn_id();
    let loop_started = Instant::now();
    let registry =
        PrimitiveToolRegistry::stage_3a_with_mcp(mcp_config_is_available(&session.project_root));
    let budget = PrimitiveLoopBudget::default();
    let turn_context = native_tool_loop_initial_messages(session, input);
    let TurnPromptContextStats {
        initial_message_count,
        history_turns,
        memory: memory_stats,
        ..
    } = turn_context.stats;

    log_loop_started(session, loop_turn_id, input, &budget);
    log_turn_prompt_context(session, initial_message_count, history_turns, &memory_stats);

    PrimitiveLoopState {
        loop_turn_id,
        loop_started,
        registry,
        budget,
        budget_state: PrimitiveLoopBudgetState::default(),
        rounds: Vec::new(),
        evidence: Vec::new(),
        memory: HarnessWorkingMemory::default(),
        provider_claim_retries: 0,
        messages: turn_context.messages,
        round_index: 0,
    }
}

fn mcp_config_is_available(project_root: &std::path::Path) -> bool {
    load_runtime_mcp_config(project_root)
        .ok()
        .flatten()
        .is_some()
}
