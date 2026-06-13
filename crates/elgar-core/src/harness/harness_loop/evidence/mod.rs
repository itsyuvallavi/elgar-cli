//! Evidence execution modules for the primitive harness loop.
//!
//! Evidence code turns validated primitive requests into verified local
//! evidence. It does not ask the model for decisions.

pub(super) mod execution;
pub(super) mod keys;
pub(super) mod mcp;
pub(super) mod render;
pub(super) mod request_args;
pub(super) mod state;
pub(super) mod summary;
