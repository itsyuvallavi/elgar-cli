//! Startup modules for the real `elgar` launch path.
//!
//! These files resolve paths/config and bridge the CLI binary into the
//! interactive terminal UI.

mod mcp_config;
mod paths;
mod provider_config;
mod terminal;

pub use mcp_config::*;
pub use paths::*;
pub use provider_config::*;
pub use terminal::*;
