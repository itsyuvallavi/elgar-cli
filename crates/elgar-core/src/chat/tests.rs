//! Tests for raw no-tool chat behavior.

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use crate::{
    chat::run_raw_chat_turn,
    event::{AssistantMessageSource, Event, ProviderOutput},
    provider::{
        ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError, ProviderRequestMetadata,
    },
    session::Session,
};

#[derive(Clone)]
struct CapturingRawProvider {
    calls: Arc<Mutex<Vec<CapturedCall>>>,
    output: ProviderOutput,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CapturedCall {
    PlainMessages {
        messages: Vec<ChatMessage>,
        has_profile: bool,
    },
    ToolMessages {
        messages: Vec<ChatMessage>,
        tools: usize,
    },
}

impl CapturingRawProvider {
    fn new(output: ProviderOutput) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            output,
        }
    }

    fn calls(&self) -> Vec<CapturedCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl ControllerProvider for CapturingRawProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("raw-test-provider", Some("raw-model".into()), "raw-1")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        panic!("raw chat should use message-based provider requests");
    }

    fn chat_messages_without_streaming_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(CapturedCall::PlainMessages {
                messages,
                has_profile: metadata.profile.is_some(),
            });
        Ok(self.output.clone())
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(CapturedCall::ToolMessages {
                messages,
                tools: tools.len(),
            });
        Ok(self.output.clone())
    }
}

fn session() -> Session {
    Session::new("raw-session", Path::new("."), Path::new("."))
}

#[test]
fn raw_chat_sends_one_no_tool_message_request() {
    let provider = CapturingRawProvider::new(ProviderOutput::new("raw answer"));
    let mut session = session();

    let result = run_raw_chat_turn(&provider, &mut session, "hello raw");

    let calls = provider.calls();
    assert_eq!(calls.len(), 1);
    match &calls[0] {
        CapturedCall::PlainMessages {
            messages,
            has_profile,
        } => {
            assert_eq!(messages, &vec![ChatMessage::user("hello raw")]);
            assert!(!has_profile);
        }
        CapturedCall::ToolMessages { .. } => panic!("raw chat attached tools"),
    }

    assert!(result.events.iter().any(|event| {
        matches!(event, Event::ProviderStarted(started) if started.request_mode.as_deref() == Some("raw_chat") && started.tool_count == Some(0))
    }));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
    assert!(result.events.iter().any(|event| matches!(
        event,
        Event::AssistantMessage(message)
            if message.source == AssistantMessageSource::Provider
                && message.content == "raw answer"
    )));
}

#[test]
fn raw_chat_does_not_record_filtered_provider_protocol_text() {
    let provider = CapturingRawProvider::new(ProviderOutput::new(
        "<tool_call>\n<function=filesystem.create_file>\n</function>\n</tool_call>",
    ));
    let mut session = session();

    let result = run_raw_chat_turn(&provider, &mut session, "show protocol");

    assert_eq!(provider.calls().len(), 1);
    assert!(!result
        .events
        .iter()
        .any(|event| matches!(event, Event::AssistantMessage(_))));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ProviderFinished(_))));
}
