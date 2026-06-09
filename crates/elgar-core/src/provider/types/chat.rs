//! Chat message types shared by provider backends.
//!
//! These messages are shared provider vocabulary for harness loops, native tool
//! results, and fallback repair calls.

use serde::{Deserialize, Deserializer, Serialize};

use super::tools::ChatToolCall;

/// Role assigned to a chat message sent to or received from a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    Developer,
    System,
    User,
    Assistant,
    Tool,
}

/// One chat message in the provider conversation format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub content: String,
    #[serde(
        default,
        alias = "reasoning_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning: Option<String>,
    #[serde(
        default,
        alias = "thinking_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub thinking: Option<String>,
    #[serde(
        default,
        deserialize_with = "deserialize_nullable_vec",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tool_calls: Vec<ChatToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    /// Creates a system instruction message.
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatRole::System, content)
    }

    /// Creates the user prompt message.
    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatRole::Assistant, content)
    }

    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            reasoning: None,
            thinking: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Tool,
            content: content.into(),
            reasoning: None,
            thinking: None,
            tool_calls: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn with_tool_calls(mut self, tool_calls: Vec<ChatToolCall>) -> Self {
        self.tool_calls = tool_calls;
        self
    }

    /// Returns reasoning/thinking text if the provider exposed it separately.
    pub fn explicit_thinking(&self) -> Option<String> {
        let thinking = [self.reasoning.as_deref(), self.thinking.as_deref()]
            .into_iter()
            .flatten()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");

        (!thinking.is_empty()).then_some(thinking)
    }
}

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_nullable_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(deserializer)?.unwrap_or_default())
}
