//! Durable harness memory built from verified session logs.
//!
//! This module reads Elgar-owned JSONL facts and builds compact indexes for
//! future turns. It does not inject memory into prompts yet.

mod index;
mod session_reader;
mod types;

pub use index::build_memory_index;
pub use session_reader::{read_session_memory_events, SessionMemoryReadError};
pub use types::{HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind};
