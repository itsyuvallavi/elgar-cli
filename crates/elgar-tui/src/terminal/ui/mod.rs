//! Terminal UI rendering modules.
//!
//! This folder owns drawing and formatting. It should not decide commands or
//! start provider requests.

pub(super) mod approval;
mod approval_card;
pub(super) mod code_syntax;
pub(super) mod code_tokens;
pub(super) mod footer;
pub(super) mod prompt;
pub(super) mod render;
pub(super) mod text;
