//! Provider event payloads and provider-owned metrics.

use serde::{Deserialize, Serialize};

use crate::{
    event::provider_output::ProviderOutput,
    provider::{ProviderBackendKind, ProviderReasoningLevel, ProviderStreamChunk},
};

/// A provider request started by the runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStarted {
    pub provider: String,
    pub request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<ProviderBackendKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ProviderReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<bool>,
}

impl ProviderStarted {
    pub fn new(provider: impl Into<String>, request_id: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            request_id: request_id.into(),
            model: None,
            request_mode: None,
            tool_count: None,
            backend: None,
            reasoning: None,
            context_length: None,
            stats: None,
        }
    }

    pub fn with_request_details(
        mut self,
        model: Option<String>,
        request_mode: impl Into<String>,
        tool_count: usize,
    ) -> Self {
        self.model = model;
        self.request_mode = Some(request_mode.into());
        self.tool_count = Some(tool_count);
        self
    }

    pub fn with_provider_profile(
        mut self,
        backend: Option<ProviderBackendKind>,
        reasoning: Option<ProviderReasoningLevel>,
        context_length: Option<u64>,
        stats: Option<bool>,
    ) -> Self {
        self.backend = backend;
        self.reasoning = reasoning;
        self.context_length = context_length;
        self.stats = stats;
        self
    }
}

/// Provider output received by the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFinished {
    pub provider: String,
    pub request_id: String,
    pub output: ProviderOutput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_timings: Option<ProviderStreamTimings>,
}

impl ProviderFinished {
    pub fn new(
        provider: impl Into<String>,
        request_id: impl Into<String>,
        output: ProviderOutput,
    ) -> Self {
        Self {
            provider: provider.into(),
            request_id: request_id.into(),
            output,
            stream_timings: None,
        }
    }

    pub fn with_stream_timings(mut self, timings: ProviderStreamTimings) -> Self {
        self.stream_timings = Some(timings);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamTimings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_reasoning_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_text_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reasoning_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_text_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chunk_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_to_text_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_chunk_to_finish_ms: Option<u64>,
    pub total_ms: u64,
}

impl ProviderStreamTimings {
    pub fn new(first_reasoning_ms: Option<u64>, first_text_ms: Option<u64>, total_ms: u64) -> Self {
        Self::from_stream_marks(
            first_reasoning_ms,
            first_text_ms,
            None,
            None,
            None,
            total_ms,
        )
    }

    pub fn from_stream_marks(
        first_reasoning_ms: Option<u64>,
        first_text_ms: Option<u64>,
        last_reasoning_ms: Option<u64>,
        last_text_ms: Option<u64>,
        last_chunk_ms: Option<u64>,
        total_ms: u64,
    ) -> Self {
        Self {
            first_reasoning_ms,
            first_text_ms,
            last_reasoning_ms,
            last_text_ms,
            last_chunk_ms,
            reasoning_to_text_ms: first_reasoning_ms
                .zip(first_text_ms)
                .map(|(reasoning, text)| text.saturating_sub(reasoning)),
            last_chunk_to_finish_ms: last_chunk_ms
                .map(|last_chunk| total_ms.saturating_sub(last_chunk)),
            total_ms,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamChunkReceived {
    pub provider: String,
    pub request_id: String,
    pub sequence: u64,
    pub chunk: ProviderStreamChunk,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_index: Option<usize>,
    #[serde(default)]
    pub canceled: bool,
}

impl ProviderStreamChunkReceived {
    pub fn new(
        provider: impl Into<String>,
        request_id: impl Into<String>,
        sequence: u64,
        chunk: ProviderStreamChunk,
    ) -> Self {
        Self {
            provider: provider.into(),
            request_id: request_id.into(),
            sequence,
            chunk,
            request_mode: None,
            loop_phase: None,
            round_index: None,
            canceled: false,
        }
    }

    pub fn with_context(
        mut self,
        request_mode: impl Into<String>,
        loop_phase: impl Into<String>,
        round_index: usize,
    ) -> Self {
        self.request_mode = Some(request_mode.into());
        self.loop_phase = Some(loop_phase.into());
        self.round_index = Some(round_index);
        self
    }
}
