//! Diagnostic and scripted CLI surfaces.
//!
//! These commands are useful for smoke tests, dogfood scripts, and provider
//! connectivity checks. They are not the normal interactive app path.

mod logs;
mod provider_smoke;
mod scripted_tui;

pub use logs::*;
pub use provider_smoke::*;
pub use scripted_tui::*;
