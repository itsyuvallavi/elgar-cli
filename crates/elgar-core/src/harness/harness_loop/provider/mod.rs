//! Provider-call modules for the primitive harness loop.
//!
//! This folder owns calls to the configured model provider. It does not execute
//! primitive tools or decide loop stop reasons.

pub(super) mod context;
pub(super) mod decision;
pub(super) mod repair;
pub(super) mod session_context;
pub(super) mod synthesis;
