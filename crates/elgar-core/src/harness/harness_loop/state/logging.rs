//! System-log helpers for the primitive harness loop.
//!
//! This module indexes focused logging helpers so the loop coordinator can
//! record runtime state without owning JSONL event shapes.

mod choice_events;
mod evidence_events;
mod memory_events;
mod permission_events;
mod provider_events;
mod round_events;

pub(in crate::harness::harness_loop) use choice_events::{
    log_loop_model_choice, log_loop_repair_finished, log_loop_repair_started,
};
pub(in crate::harness::harness_loop) use evidence_events::log_loop_evidence;
pub(in crate::harness::harness_loop) use memory_events::{
    log_harness_duplicate_rejected, log_harness_memory_snapshot,
};
pub(in crate::harness::harness_loop) use permission_events::{
    log_harness_approval_requested, log_permission_decision,
};
pub(in crate::harness::harness_loop) use provider_events::{
    log_decision_context, log_provider_call_failed, log_provider_call_finished,
    log_provider_call_started, log_turn_prompt_context,
};
pub(in crate::harness::harness_loop) use round_events::{
    log_loop_finished, log_loop_round_finished, log_loop_round_started,
};
