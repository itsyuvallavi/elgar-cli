//! Terminal UI rendering modules.
//!
//! This folder owns drawing and formatting. It should not decide commands or
//! start provider requests.

pub(super) mod approval;
pub(super) mod approval_action;
mod approval_card;
mod approval_card_style;
pub(super) mod code_syntax;
pub(super) mod code_tokens;
pub(crate) mod event_blocks;
pub(crate) mod execution_result;
pub(super) mod footer;
pub(super) mod prompt;
pub(super) mod render;
pub(crate) mod section_render;
pub(crate) mod sections;
pub(super) mod text;
pub(crate) mod transcript_print;
