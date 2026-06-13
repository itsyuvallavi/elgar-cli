//! Pane module entry point.
//!
//! Panes are persistent pieces of TUI state: conversation history, input,
//! status, copy buffer, and provider reasoning display.

mod conversation;
mod event_rendering;
pub(crate) mod provider_reasoning;
mod status;

#[cfg(test)]
mod tests;

pub(crate) use conversation::ConversationLineStyle;
pub use conversation::ConversationPane;
pub use status::{CopyArea, InputArea, StatusLine};
