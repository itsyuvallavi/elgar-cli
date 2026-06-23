//! Shared provider vocabulary.
//!
//! This folder describes the contract between the harness, the TUI, tests, and
//! concrete providers such as LM Studio.

mod chat;
mod controller;
mod error;
mod metadata;
mod profile;
mod request;
mod stream;
mod tools;

pub use chat::{ChatMessage, ChatRole};
pub use controller::ControllerProvider;
pub use error::{ProviderError, ProviderErrorBody, ProviderErrorKind, ProviderErrorResponse};
pub use metadata::ProviderRequestMetadata;
pub use profile::{ProviderBackendKind, ProviderReasoningLevel, ProviderRequestProfile};
pub use request::{
    ChatChoice, ChatRequest, ChatResponse, ChatStreamOptions, ChatTemplateKwargs, ChatUsage,
};
pub use stream::ProviderStreamChunk;
pub use tools::{
    ChatToolCall, ChatToolCallDelta, ChatToolCallFunction, ChatToolDefinition,
    ChatToolFunctionDefinition, ChatToolType,
};
