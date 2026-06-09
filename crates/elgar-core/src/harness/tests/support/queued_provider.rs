//! Shared test support for harness tests.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use crate::{
    event::ProviderOutput,
    provider::{
        ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError, ProviderRequestMetadata,
    },
};

#[derive(Clone)]
pub(in crate::harness::tests) struct QueuedProvider {
    pub(in crate::harness::tests) outputs: Arc<Mutex<VecDeque<ProviderOutput>>>,
    pub(in crate::harness::tests) calls: Arc<Mutex<Vec<Vec<ChatMessage>>>>,
    pub(in crate::harness::tests) tool_calls: Arc<Mutex<Vec<Vec<ChatToolDefinition>>>>,
}

impl QueuedProvider {
    pub(in crate::harness::tests) fn new(outputs: Vec<&str>) -> Self {
        Self::new_outputs(
            outputs
                .into_iter()
                .map(ProviderOutput::new)
                .collect::<Vec<_>>(),
        )
    }

    pub(in crate::harness::tests) fn new_outputs(outputs: Vec<ProviderOutput>) -> Self {
        Self {
            outputs: Arc::new(Mutex::new(outputs.into_iter().collect::<VecDeque<_>>())),
            calls: Arc::new(Mutex::new(Vec::new())),
            tool_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ControllerProvider for QueuedProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("queued-test", Some("test-model".into()), "queued-request")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        panic!("primitive harness loop should use message calls")
    }

    fn chat_messages_without_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.calls.lock().expect("calls lock").push(messages);
        Ok(self
            .outputs
            .lock()
            .expect("outputs lock")
            .pop_front()
            .expect("queued provider output"))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        self.calls.lock().expect("calls lock").push(messages);
        self.tool_calls.lock().expect("tool calls lock").push(tools);
        Ok(self
            .outputs
            .lock()
            .expect("outputs lock")
            .pop_front()
            .expect("queued provider output"))
    }
}
