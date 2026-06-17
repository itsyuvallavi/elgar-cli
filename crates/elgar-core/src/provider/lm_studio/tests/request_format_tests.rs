//! LM Studio request formatting tests.

use serde_json::json;

use super::super::{
    elgar_controller_messages, elgar_controller_messages_for_config, format_chat_request,
    format_chat_request_body, parse_chat_response_json_with_metrics, ChatMessage, LmStudioProvider,
    ProviderConfig,
};
use crate::event::ProviderMetrics;
use crate::provider::{
    ChatToolDefinition, ControllerProvider, ProviderBackendKind, ProviderReasoningLevel,
    ProviderRequestProfile,
};

#[test]
fn formats_non_streaming_openai_compatible_chat_request() {
    let config = ProviderConfig::lm_studio("loaded-model");

    let request = format_chat_request(
        &config,
        vec![
            ChatMessage::system("You suggest only."),
            ChatMessage::user("Summarize this file."),
        ],
    )
    .unwrap();

    assert_eq!(request.model, "loaded-model");
    assert!(!request.stream);
    assert_eq!(request.messages.len(), 2);
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        json!({
            "model": "loaded-model",
            "messages": [
                { "role": "system", "content": "You suggest only." },
                { "role": "user", "content": "Summarize this file." }
            ],
            "stream": false
        })
    );
}

#[test]
fn formats_opt_in_streaming_openai_compatible_chat_request() {
    let config = ProviderConfig {
        stream: true,
        ..ProviderConfig::lm_studio("loaded-model")
    };

    let request = format_chat_request(&config, vec![ChatMessage::user("hello")]).unwrap();

    assert!(request.stream);
    assert_eq!(
        serde_json::to_value(&request).unwrap(),
        json!({
            "model": "loaded-model",
            "messages": [
                { "role": "user", "content": "hello" }
            ],
            "stream": true
        })
    );
}

#[test]
fn openai_compatible_request_keeps_stats_and_omits_non_openai_profile_fields() {
    let config = ProviderConfig::lm_studio("loaded-model");
    let profile = ProviderRequestProfile {
        backend: ProviderBackendKind::OpenAiChatCompletions,
        stream: Some(false),
        reasoning: Some(ProviderReasoningLevel::Off),
        context_length: Some(8000),
        stats: Some(true),
        stateful: None,
    };

    let (request, body) = super::super::format_chat_request_body_with_tools_and_profile(
        &config,
        vec![ChatMessage::user("hello")],
        Vec::new(),
        Some(&profile),
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&body).unwrap();

    assert_eq!(request.reasoning, None);
    assert_eq!(request.stats, Some(true));
    assert!(value.get("reasoning").is_none());
    assert!(value.get("context_length").is_none());
    assert_eq!(value["stats"], true);
    assert!(value.get("tools").is_none());
    assert!(value.get("tool_choice").is_none());
}

#[test]
fn openai_compatible_request_omits_stats_without_profile_opt_in() {
    let config = ProviderConfig::lm_studio("loaded-model");

    let (_request, body) = super::super::format_chat_request_body_with_tools_and_profile(
        &config,
        vec![ChatMessage::user("hello")],
        Vec::new(),
        None,
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&body).unwrap();

    assert!(value.get("stats").is_none());
}

#[test]
fn streaming_stats_request_includes_openai_usage_stream_options() {
    let config = ProviderConfig::lm_studio("loaded-model");
    let profile = ProviderRequestProfile {
        backend: ProviderBackendKind::OpenAiChatCompletions,
        stream: Some(true),
        reasoning: None,
        context_length: None,
        stats: Some(true),
        stateful: None,
    };

    let (request, body) = super::super::format_chat_request_body_with_tools_and_profile(
        &config,
        vec![ChatMessage::user("hello")],
        Vec::new(),
        Some(&profile),
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&body).unwrap();

    assert!(request.stream);
    assert_eq!(value["stats"], true);
    assert_eq!(value["stream_options"]["include_usage"], true);
}

#[test]
fn tool_enabled_openai_request_sends_tools_without_tool_choice() {
    let config = ProviderConfig::lm_studio("loaded-model");
    let tool = ChatToolDefinition::function(
        "read",
        "Read a project file.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            },
            "required": ["path"]
        }),
    );

    let (_request, body) = super::super::format_chat_request_body_with_tools_and_profile(
        &config,
        vec![ChatMessage::user("show me package.json")],
        vec![tool],
        None,
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&body).unwrap();

    assert_eq!(value["tools"][0]["function"]["name"], "read");
    assert!(value.get("tool_choice").is_none());
}

#[test]
fn controller_provider_messages_keep_terminal_answers_short_and_readable() {
    let messages = elgar_controller_messages("what can you do?");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, crate::provider::ChatRole::System);
    assert!(messages[0].content.len() <= 420);
    assert!(messages[0].content.is_ascii());
    assert!(!messages[0].content.contains('\n'));
    assert!(messages[0].content.contains("Answer briefly"));
    assert!(messages[0].content.contains("terminal-friendly"));
    assert!(messages[0].content.contains("no tables unless asked"));
    assert!(messages[0].content.contains("Speak as Elgar"));
    assert!(messages[0].content.contains("Suggest content only"));
    assert!(messages[0]
        .content
        .contains("Do not write 'Proposed actions'"));
    assert!(messages[0]
        .content
        .contains("unless a controller action is pending"));
    assert!(messages[0]
        .content
        .contains("Never claim you created/edited/ran anything"));
    assert!(messages[0]
        .content
        .contains("Provider text never proves files changed or commands ran"));
    assert!(messages[0].content.contains("copy/paste"));
    assert_eq!(messages[1], ChatMessage::user("what can you do?"));
}

#[test]
fn controller_provider_messages_use_configured_developer_role() {
    let config = ProviderConfig {
        compatibility: crate::provider::ProviderCompatibility {
            supports_developer_role: Some(true),
            ..Default::default()
        },
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let messages = elgar_controller_messages_for_config(&config, "what can you do?");
    let request = format_chat_request(&config, messages).unwrap();

    assert_eq!(
        request.messages[0].role,
        crate::provider::ChatRole::Developer
    );
    assert_eq!(
        serde_json::to_value(&request).unwrap()["messages"][0]["role"],
        "developer"
    );
}

#[test]
fn controller_provider_request_for_short_capability_answer_stays_compact() {
    let config = ProviderConfig::lm_studio("loaded-model");
    let (request, body) =
        format_chat_request_body(&config, elgar_controller_messages("what can you do?")).unwrap();
    let metrics =
        super::super::openai::metrics_for_request("request-compact", &request, body.len(), None);

    assert_eq!(request.messages.len(), 2);
    assert_eq!(metrics.message_count, 2);
    assert!(metrics.serialized_request_bytes <= 650);
    assert_eq!(metrics.serialized_request_bytes, body.len());
}

#[test]
fn serialized_request_byte_count_matches_request_body() {
    let config = ProviderConfig {
        stream: true,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let (request, body) = format_chat_request_body(
        &config,
        vec![
            ChatMessage::system("You suggest only."),
            ChatMessage::user("hello"),
        ],
    )
    .unwrap();
    let metrics =
        super::super::openai::metrics_for_request("request-1", &request, body.len(), None);

    assert_eq!(metrics.request_id, "request-1");
    assert_eq!(metrics.model.as_deref(), Some("loaded-model"));
    assert!(metrics.stream);
    assert_eq!(metrics.message_count, 2);
    assert_eq!(metrics.serialized_request_bytes, body.len());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["stream"],
        true
    );
}

#[test]
fn openai_compatible_usage_is_propagated_into_provider_metrics() {
    let metrics = ProviderMetrics::new(
        "request-usage",
        Some("loaded-model".to_string()),
        false,
        1,
        123,
    );
    let output = parse_chat_response_json_with_metrics(
        r#"{
            "id": "chatcmpl-local",
            "model": "loaded-model",
            "choices": [
                {
                    "message": { "role": "assistant", "content": "Done." },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 7,
                "completion_tokens": 3,
                "total_tokens": 10
            }
        }"#,
        Some(metrics),
    )
    .unwrap();

    let metrics = output.metrics.unwrap();
    let usage = metrics.usage.unwrap();
    assert_eq!(output.text, "Done.");
    assert_eq!(metrics.request_id, "request-usage");
    assert_eq!(usage.prompt_tokens, Some(7));
    assert_eq!(usage.completion_tokens, Some(3));
    assert_eq!(usage.total_tokens, Some(10));
}

#[test]
fn lm_studio_request_metadata_ids_are_unique_across_turns() {
    let provider = LmStudioProvider::new(ProviderConfig::lm_studio("loaded-model"));

    let first = provider.request_metadata();
    let second = provider.request_metadata();

    assert_ne!(first.request_id, second.request_id);
    assert!(first.request_id.starts_with("lm-studio-request-"));
    assert!(second.request_id.starts_with("lm-studio-request-"));
}
