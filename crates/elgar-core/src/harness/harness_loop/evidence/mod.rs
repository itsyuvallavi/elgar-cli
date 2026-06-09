//! Evidence execution modules for the primitive harness loop.
//!
//! Evidence code turns validated primitive requests into verified local
//! evidence. It does not ask the model for decisions.

pub(super) mod execution;
pub(super) mod state;
pub(super) mod summary;
