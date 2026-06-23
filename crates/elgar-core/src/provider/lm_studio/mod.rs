//! Wires Elgar's LM Studio provider together.
//!
//! This file owns public helper exports and request id generation. The concrete
//! provider backend, request formatting, parsing, and OpenAI-compatible calls
//! live in sibling files in this folder.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        types::{ChatMessage, ProviderError, ProviderStreamChunk},
    },
};

mod backend;
mod format;
mod openai;
mod parse;

pub use backend::LmStudioProvider;
use openai::{chat_lm_studio_streaming_with_request_id, chat_lm_studio_with_request_id};

#[cfg(test)]
pub(crate) use format::elgar_controller_messages;
#[cfg(test)]
pub(crate) use format::elgar_controller_messages_for_config;
#[cfg(test)]
pub(crate) use format::format_chat_request_body_with_tools_and_profile;
pub use format::{
    format_chat_request, format_chat_request_body, format_chat_request_body_with_tools,
    format_chat_request_with_tools,
};
pub use parse::{
    parse_chat_response_json, parse_chat_response_json_with_metrics, parse_chat_stream_chunks,
    parse_chat_stream_line, parse_chat_stream_response, parse_provider_error_json,
};

/// Explicit, opt-in live call for LM Studio/OpenAI-compatible local servers.
///
/// This is only used by explicit smoke/live-provider paths. Normal controller
/// behavior and tests remain no-network by default.
pub fn chat_lm_studio(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<ProviderOutput, ProviderError> {
    let request_id = next_lm_studio_request_id();
    chat_lm_studio_with_request_id(config, messages, &request_id, None)
}

pub fn chat_lm_studio_streaming(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> Result<ProviderOutput, ProviderError> {
    let request_id = next_lm_studio_request_id();
    chat_lm_studio_streaming_with_request_id(config, messages, &request_id, None, on_chunk)
}

static LM_STUDIO_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(super) fn next_lm_studio_request_id() -> String {
    let sequence = LM_STUDIO_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("lm-studio-request-{sequence}")
}

#[cfg(test)]
mod tests;
