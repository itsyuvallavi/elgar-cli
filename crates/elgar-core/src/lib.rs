//! Elgar core library.
//!
//! Core owns provider communication, session/events, rendering helpers, token
//! accounting, and local logs used by CLI/TUI surfaces.

pub mod context;
pub mod event;
pub mod harness;
pub mod logs;
pub mod mcp;
pub mod provider;
mod provider_visible;
pub use logs::sessions::{session_log_directory, session_log_path};
pub use logs::system::{log_directory, log_path};
pub use provider_visible::provider_visible_text_from_text_only_output;
pub mod renderer;
pub mod session;
pub mod token_accounting;

pub const CORE_PHILOSOPHY: &str = "Model reasons. Runtime routes. Action gate enforces. Filesystem confirms. UI reports. Tests protect. Extensions wait.";
