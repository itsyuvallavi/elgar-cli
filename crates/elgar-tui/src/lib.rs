//! Elgar terminal UI crate.
//!
//! This crate turns core session events into a terminal experience: input,
//! panes, command handling, startup text, and live provider progress.

mod code_blocks;
mod input;
pub mod layout;
mod markdown;
pub mod panes;
pub mod shell;
mod startup;
pub mod terminal;
mod theme;
mod turn_metrics;

pub use layout::LayoutRegion;
pub use panes::{ConversationPane, InputArea, StatusLine};
pub use shell::TuiShell;
pub use startup::StartupMcpStatus;
pub use terminal::{
    default_shell_text, run_terminal_shell, run_terminal_shell_at,
    run_terminal_shell_at_with_mcp_status, run_terminal_shell_with_lm_studio_provider,
    run_terminal_shell_with_lm_studio_provider_and_mcp_at,
    run_terminal_shell_with_lm_studio_provider_at,
};
