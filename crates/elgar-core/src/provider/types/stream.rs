//! Provider streaming chunk types.
//!
//! Streaming backends can send reasoning and final response text separately.

use serde::{Deserialize, Serialize};

use super::tools::ChatToolCallDelta;

/// A live streaming update from the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamChunk {
    Reasoning(String),
    Text(String),
    ToolCallDelta(ChatToolCallDelta),
}
