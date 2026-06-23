//! Shared loop state, logging, and result types.
//!
//! This folder contains support code used by control, provider, and evidence
//! modules. It should stay free of provider calls and primitive execution.

pub(super) mod budget;
pub(super) mod listing_memory;
pub(super) mod logging;
pub(super) mod memory;
pub mod types;
