//! Model-choice protocol exports for the primitive harness.
//!
//! This file is intentionally only a module boundary. Provider calls belong in
//! `harness_loop/decision.rs`; primitive execution belongs in
//! `harness_loop/evidence.rs`.

mod contracts;
mod json_extract;
mod parsing;
mod policy;
mod prose_guard;
mod types;
mod validation;

pub use contracts::{loop_decision_contract, model_choice_contract};
pub use parsing::{parse_model_choice, parse_model_choice_with_registry};
pub use policy::MAX_TOOL_CALL_BATCH;
pub use types::{
    EvidenceDepth, ModelChoice, ModelChoiceTurnError, StructuredRequestKind,
    StructuredRequestValidationError, ValidatedStructuredRequest,
};
