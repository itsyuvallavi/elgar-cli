//! LM Studio provider backend implementation.
//!
//! This module owns the public `LmStudioProvider` type and its
//! `ControllerProvider` implementation. Request formatting, parsing, and HTTP
//! calls stay in sibling modules.

use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        types::{
            ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata, ProviderStreamChunk,
        },
        ProviderCancelToken,
    },
};

use super::{
    format::elgar_controller_messages_for_config,
    next_lm_studio_request_id,
    openai::{
        chat_lm_studio_streaming_with_request_id,
        chat_lm_studio_streaming_with_request_id_cancelable, chat_lm_studio_with_request_id,
        chat_lm_studio_with_request_id_cancelable,
        chat_lm_studio_with_tools_streaming_with_request_id_cancelable,
        chat_lm_studio_with_tools_with_request_id_cancelable, openai_chat_profile,
    },
};

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
        super::chat_lm_studio(
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
        chat_lm_studio_with_tools_with_request_id_cancelable(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
            &ProviderCancelToken::new(),
        )
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_with_tools_with_request_id_cancelable(
            &self.config,
            messages,
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
            &ProviderCancelToken::new(),
        )
    }

    fn chat_messages_with_tools_with_metadata_cancelable(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
        cancel: &ProviderCancelToken,
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_with_tools_with_request_id_cancelable(
            &self.config,
            messages,
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
            cancel,
        )
    }

    fn chat_messages_with_tools_streaming_with_metadata_cancelable(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        cancel: &ProviderCancelToken,
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_with_tools_streaming_with_request_id_cancelable(
            &self.config,
            messages,
            &metadata.request_id,
            tools,
            metadata.profile.as_ref(),
            on_chunk,
            cancel,
        )
    }

    fn chat_messages_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        if self.config.stream {
            let mut ignore_chunks = |_chunk: ProviderStreamChunk| {};
            chat_lm_studio_streaming_with_request_id(
                &self.config,
                messages,
                &metadata.request_id,
                openai_chat_profile(metadata.profile.as_ref()).as_ref(),
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
        let mut config = self.config.clone();
        config.stream = false;
        let mut profile = openai_chat_profile(metadata.profile.as_ref());
        if let Some(profile) = profile.as_mut() {
            profile.stream = Some(false);
        }
        chat_lm_studio_with_request_id(&config, messages, &metadata.request_id, profile.as_ref())
    }

    fn chat_messages_without_streaming_with_metadata_cancelable(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        cancel: &ProviderCancelToken,
    ) -> Result<ProviderOutput, ProviderError> {
        let mut config = self.config.clone();
        config.stream = false;
        let mut profile = openai_chat_profile(metadata.profile.as_ref());
        if let Some(profile) = profile.as_mut() {
            profile.stream = Some(false);
        }
        chat_lm_studio_with_request_id_cancelable(
            &config,
            messages,
            &metadata.request_id,
            profile.as_ref(),
            cancel,
        )
    }

    fn chat_messages_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        let mut config = self.config.clone();
        config.stream = true;

        let profile = openai_chat_profile(metadata.profile.as_ref());
        chat_lm_studio_streaming_with_request_id(
            &config,
            messages,
            &metadata.request_id,
            profile.as_ref(),
            on_chunk,
        )
    }

    fn chat_messages_streaming_with_metadata_cancelable(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        cancel: &ProviderCancelToken,
    ) -> Result<ProviderOutput, ProviderError> {
        let mut config = self.config.clone();
        config.stream = true;

        chat_lm_studio_streaming_with_request_id_cancelable(
            &config,
            messages,
            &metadata.request_id,
            openai_chat_profile(metadata.profile.as_ref()).as_ref(),
            on_chunk,
            cancel,
        )
    }

    fn chat_stream(
        &self,
        prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        super::chat_lm_studio_streaming(
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
            openai_chat_profile(metadata.profile.as_ref()).as_ref(),
            on_chunk,
        )
    }
}
