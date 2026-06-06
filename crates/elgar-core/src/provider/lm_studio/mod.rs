//! Wires Elgar's LM Studio provider together.
//!
//! This file owns the public provider type and backend selection. The concrete
//! request formatting, parsing, native calls, and OpenAI-compatible calls live
//! in the sibling files in this folder.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        types::{
            ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata, ProviderStreamChunk,
        },
    },
};

mod format;
mod native;
mod openai;
mod parse;

use format::elgar_controller_messages_for_config;
use native::{
    chat_lm_studio_native_no_tool_with_request_id, messages_are_native_no_tool_safe,
    profile_allows_native_no_tool,
};
use openai::{
    chat_lm_studio_streaming_with_request_id, chat_lm_studio_with_request_id,
    chat_lm_studio_with_tools_with_request_id, openai_chat_profile,
};

#[cfg(test)]
pub(crate) use format::elgar_controller_messages;
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
    chat_lm_studio_streaming_with_request_id(config, messages, &request_id, on_chunk)
}

/// Explicit opt-in LM Studio provider backend for controller live mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LmStudioProvider {
    pub config: ProviderConfig,
}

impl LmStudioProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self { config }
    }
}

impl ControllerProvider for LmStudioProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            self.config.provider.clone(),
            self.config.model.clone(),
            next_lm_studio_request_id(),
        )
    }

    fn request_metadata_for_mode(&self, request_mode: &str) -> ProviderRequestMetadata {
        self.request_metadata()
            .with_profile(self.config.request_profile_for_mode(request_mode))
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
        )
    }

    fn chat_with_metadata(
        &self,
        prompt: &str,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        let profile = openai_chat_profile(metadata.profile.as_ref());
        chat_lm_studio_with_request_id(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            &metadata.request_id,
            profile.as_ref(),
        )
    }

    fn chat_with_tools_with_metadata(
        &self,
        prompt: &str,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_with_tools_with_request_id(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
        )
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_with_tools_with_request_id(
            &self.config,
            messages,
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
        )
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        if profile_allows_native_no_tool(metadata.profile.as_ref())
            && messages_are_native_no_tool_safe(&messages)
        {
            return chat_lm_studio_native_no_tool_with_request_id(
                &self.config,
                messages,
                &metadata.request_id,
                metadata.profile.as_ref().expect("profile checked"),
            );
        }

        if self.config.stream {
            let mut ignore_chunks = |_chunk: ProviderStreamChunk| {};
            chat_lm_studio_streaming_with_request_id(
                &self.config,
                messages,
                &metadata.request_id,
                &mut ignore_chunks,
            )
        } else {
            let profile = openai_chat_profile(metadata.profile.as_ref());
            chat_lm_studio_with_request_id(
                &self.config,
                messages,
                &metadata.request_id,
                profile.as_ref(),
            )
        }
    }

    fn chat_messages_without_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        if profile_allows_native_no_tool(metadata.profile.as_ref())
            && messages_are_native_no_tool_safe(&messages)
        {
            let mut profile = metadata
                .profile
                .clone()
                .expect("profile checked after native backend check");
            profile.stream = Some(false);
            return chat_lm_studio_native_no_tool_with_request_id(
                &self.config,
                messages,
                &metadata.request_id,
                &profile,
            );
        }

        let mut config = self.config.clone();
        config.stream = false;
        let mut profile = openai_chat_profile(metadata.profile.as_ref());
        if let Some(profile) = profile.as_mut() {
            profile.stream = Some(false);
        }
        chat_lm_studio_with_request_id(&config, messages, &metadata.request_id, profile.as_ref())
    }

    fn chat_messages_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        let mut config = self.config.clone();
        config.stream = true;

        chat_lm_studio_streaming_with_request_id(&config, messages, &metadata.request_id, on_chunk)
    }

    fn chat_stream(
        &self,
        prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_streaming(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            on_chunk,
        )
    }

    fn chat_stream_with_metadata(
        &self,
        prompt: &str,
        metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_streaming_with_request_id(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            &metadata.request_id,
            on_chunk,
        )
    }
}

static LM_STUDIO_REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_lm_studio_request_id() -> String {
    let sequence = LM_STUDIO_REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("lm-studio-request-{sequence}")
}

#[cfg(test)]
mod tests;
