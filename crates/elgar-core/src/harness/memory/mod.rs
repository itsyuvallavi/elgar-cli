//! Durable harness memory built from verified session logs.
//!
//! This module reads Elgar-owned JSONL facts, builds compact indexes, and
//! renders advisory prompt text for cross-turn harness turns.

mod budget;
mod index;
mod render;
mod session_reader;
mod types;

pub use budget::{HarnessMemoryPromptBudget, RenderedMemoryStats};
pub use index::build_memory_index;
pub use render::{render_verified_memory_for_prompt_with_budget, RenderedMemoryPrompt};
pub use session_reader::{read_session_memory_events, SessionMemoryReadError};
pub use types::{HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind};
