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
    logs::system::{append_log_event, LogInput, LogPhase},
    mcp::config::{load_runtime_mcp_config, RuntimeMcpConfig},
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
    let mcp_config = load_runtime_mcp_config(&session.project_root);
    let mcp_available = matches!(mcp_config.as_ref(), Ok(Some(_)));
    let registry = PrimitiveToolRegistry::stage_3a_with_mcp(mcp_available);
    let budget = PrimitiveLoopBudget::default();
    let turn_context = native_tool_loop_initial_messages(session, input);
    let TurnPromptContextStats {
        initial_message_count,
        history_turns,
        memory: memory_stats,
        ..
    } = turn_context.stats;

    log_loop_started(session, loop_turn_id, input, &budget);
    log_mcp_status(session, mcp_config.as_ref());
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

fn log_mcp_status(
    session: &Session,
    runtime: Result<&Option<RuntimeMcpConfig>, &crate::mcp::config::McpConfigError>,
) {
    let metadata = match runtime {
        Ok(Some(runtime)) => {
            let server_ids = runtime.config.servers.keys().cloned().collect::<Vec<_>>();
            serde_json::json!({
                "mcp_active": true,
                "mcp_tool_exposed": true,
                "source_path": runtime.source_path.display().to_string(),
                "server_count": server_ids.len(),
                "server_ids": server_ids
            })
        }
        Ok(None) => serde_json::json!({
            "mcp_active": false,
            "mcp_tool_exposed": false,
            "server_count": 0,
            "server_ids": []
        }),
        Err(error) => serde_json::json!({
            "mcp_active": false,
            "mcp_tool_exposed": false,
            "server_count": 0,
            "server_ids": [],
            "error": error.to_string()
        }),
    };

    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "initialize_primitive_loop",
            "harness_mcp_status",
        )
        .with_metadata(metadata),
    );
}
