//! Active tests for the LM Studio provider.
//!
//! These tests cover the raw/no-tool provider path we still use: request
//! formatting, native chat, OpenAI-compatible chat, parsing, streaming, usage,
//! and local timeout behavior.

use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use super::{
    chat_lm_studio, chat_lm_studio_streaming, elgar_controller_messages,
    elgar_controller_messages_for_config, format_chat_request, format_chat_request_body,
    parse_chat_response_json, parse_chat_response_json_with_metrics, parse_chat_stream_chunks,
    parse_chat_stream_response, parse_provider_error_json, ChatMessage, LmStudioProvider,
    ProviderConfig,
};
use crate::event::ProviderMetrics;
use crate::provider::{
    http::parse_http_response, ControllerProvider, ProviderBackendKind, ProviderErrorKind,
    ProviderReasoningLevel, ProviderRequestProfile, ProviderStreamChunk,
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
fn openai_compatible_request_omits_native_only_profile_fields() {
    let config = ProviderConfig::lm_studio("loaded-model");
    let profile = ProviderRequestProfile {
        backend: ProviderBackendKind::OpenAiChatCompletions,
        stream: Some(false),
        reasoning: Some(ProviderReasoningLevel::Off),
        context_length: Some(8000),
        stats: Some(true),
        stateful: None,
    };

    let (request, body) = super::format_chat_request_body_with_tools_and_profile(
        &config,
        vec![ChatMessage::user("hello")],
        Vec::new(),
        Some(&profile),
    )
    .unwrap();
    let value = serde_json::from_str::<serde_json::Value>(&body).unwrap();

    assert_eq!(request.reasoning, None);
    assert!(value.get("reasoning").is_none());
    assert!(value.get("context_length").is_none());
    assert!(value.get("stats").is_none());
    assert!(value.get("tools").is_none());
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
    let metrics = super::openai::metrics_for_request("request-compact", &request, body.len(), None);

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
    let metrics = super::openai::metrics_for_request("request-1", &request, body.len(), None);

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

#[test]
fn request_formatting_requires_model_and_message() {
    let missing_model =
        format_chat_request(&ProviderConfig::default(), vec![ChatMessage::user("hello")])
            .unwrap_err();
    assert_eq!(missing_model.kind, ProviderErrorKind::Configuration);
    assert!(missing_model.message.contains("model"));

    let missing_message =
        format_chat_request(&ProviderConfig::lm_studio("loaded-model"), Vec::new()).unwrap_err();
    assert_eq!(missing_message.kind, ProviderErrorKind::Configuration);
    assert!(missing_message.message.contains("message"));
}

#[test]
fn parses_first_non_empty_assistant_response_as_provider_output() {
    let output = parse_chat_response_json(
        r#"{
            "id": "chatcmpl-local",
            "model": "loaded-model",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "  Suggested next step.  ",
                        "tool_calls": null
                    },
                    "finish_reason": "stop"
                }
            ],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 4,
                "total_tokens": 14
            }
        }"#,
    )
    .unwrap();

    assert_eq!(output.text, "Suggested next step.");
    assert_eq!(output.thinking, None);
}

#[test]
fn parses_explicit_thinking_separately_from_assistant_response() {
    let output = parse_chat_response_json(
        r#"{
            "id": "chatcmpl-local",
            "model": "loaded-model",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "reasoning_content": "  I should explain the next step.  ",
                        "thinking": "Keep it concise.",
                        "content": "  Suggested next step.  "
                    },
                    "finish_reason": "stop"
                }
            ]
        }"#,
    )
    .unwrap();

    assert_eq!(output.text, "Suggested next step.");
    assert_eq!(
        output.thinking.as_deref(),
        Some("I should explain the next step.\n\nKeep it concise.")
    );
}

#[test]
fn parses_lm_studio_streaming_chunks_as_provider_suggestion() {
    let output = parse_chat_stream_response(
        r#"data: {"choices":[{"delta":{"role":"assistant"}}]}
data: {"choices":[{"delta":{"content":"  Suggested"}}]}
data: {"choices":[{"delta":{"content":" next"}}]}
data: {"choices":[{"delta":{"content":" step.  "},"finish_reason":"stop"}]}
data: [DONE]
"#,
    )
    .unwrap();

    assert_eq!(output.text, "Suggested next step.");
    assert_eq!(output.thinking, None);
}

#[test]
fn streaming_chunk_parser_exposes_reasoning_and_text_separately() {
    let chunks = parse_chat_stream_chunks(
        r#"data: {"choices":[{"delta":{"reasoning_content":"Need greet."}}]}
data: {"choices":[{"delta":{"content":"Hello"}}]}
data: [DONE]
"#,
    )
    .unwrap();

    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("Need greet.".to_string()),
            ProviderStreamChunk::Text("Hello".to_string())
        ]
    );
}

#[test]
fn parses_streaming_thinking_separately_from_provider_text() {
    let output = parse_chat_stream_response(
        r#"data: {"choices":[{"delta":{"reasoning_content":"  Think"}}]}
data: {"choices":[{"delta":{"thinking":" first.  "}}]}
data: {"choices":[{"delta":{"content":"Answer."}}]}
data: [DONE]
"#,
    )
    .unwrap();

    assert_eq!(output.text, "Answer.");
    assert_eq!(output.thinking.as_deref(), Some("Think first."));
}

#[test]
fn streaming_parser_reports_malformed_or_empty_payloads() {
    let malformed = parse_chat_stream_response("data: {not json").unwrap_err();
    assert_eq!(malformed.kind, ProviderErrorKind::ResponseParse);

    let empty = parse_chat_stream_response(
        r#"data: {"choices":[{"delta":{"content":"   "}}]}
data: [DONE]
"#,
    )
    .unwrap_err();
    assert_eq!(empty.kind, ProviderErrorKind::EmptyResponse);
}

#[test]
fn response_parse_reports_malformed_or_empty_payloads() {
    let malformed = parse_chat_response_json("{not json").unwrap_err();
    assert_eq!(malformed.kind, ProviderErrorKind::ResponseParse);

    let empty = parse_chat_response_json(
        r#"{
            "choices": [
                { "message": { "role": "assistant", "content": "   " } }
            ]
        }"#,
    )
    .unwrap_err();
    assert_eq!(empty.kind, ProviderErrorKind::EmptyResponse);
}

#[test]
fn maps_openai_compatible_error_payload() {
    let error = parse_provider_error_json(
        Some(400),
        r#"{
            "error": {
                "message": "model is not loaded",
                "type": "invalid_request_error",
                "code": "model_not_found"
            }
        }"#,
    );

    assert_eq!(error.kind, ProviderErrorKind::Provider);
    assert_eq!(error.status_code, Some(400));
    assert_eq!(error.code.as_deref(), Some("model_not_found"));
    assert_eq!(error.message, "model is not loaded");
}

#[test]
fn malformed_provider_error_payload_maps_to_parse_error_with_status() {
    let error = parse_provider_error_json(Some(500), "not json");

    assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
    assert_eq!(error.status_code, Some(500));
    assert!(error.message.contains("expected"));
}

#[test]
fn http_provider_error_response_maps_status_and_payload() {
    let response = parse_http_response(
        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\n\r\n\
        {\"error\":{\"message\":\"model missing\",\"code\":\"model_not_found\"}}",
    )
    .unwrap();

    assert!(!response.status_code.is_success());
    let error = parse_provider_error_json(Some(response.status_code.as_u16()), &response.body);

    assert_eq!(error.kind, ProviderErrorKind::Provider);
    assert_eq!(error.status_code, Some(404));
    assert_eq!(error.code.as_deref(), Some("model_not_found"));
    assert_eq!(error.message, "model missing");
}

#[test]
fn non_streaming_message_request_overrides_streaming_config() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let bytes_read = stream.read(&mut request).unwrap();
        sender
            .send(String::from_utf8_lossy(&request[..bytes_read]).to_string())
            .unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
                {\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"Hello.\"}}],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2,\"total_tokens\":7}}",
            )
            .unwrap();
    });

    let provider = LmStudioProvider::new(ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    });
    let metadata = provider.request_metadata();
    let output = provider
        .chat_messages_without_streaming_with_metadata(vec![ChatMessage::user("hello")], &metadata)
        .unwrap();

    server.join().unwrap();
    let request = receiver.recv().unwrap();
    assert!(request.contains(r#""stream":false"#));
    assert_eq!(output.metrics.unwrap().usage.unwrap().total_tokens, Some(7));
}

#[test]
fn native_no_tool_profile_uses_lm_studio_native_chat_and_records_stats() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 8192];
        let bytes_read = stream.read(&mut request).unwrap();
        sender
            .send(String::from_utf8_lossy(&request[..bytes_read]).to_string())
            .unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
                {\"output\":[{\"type\":\"reasoning\",\"content\":\"Need answer.\"},{\"type\":\"message\",\"content\":\"Build passed.\"}],\"stats\":{\"input_tokens\":10,\"total_output_tokens\":5,\"reasoning_output_tokens\":2,\"tokens_per_second\":50.5,\"time_to_first_token_seconds\":0.42}}",
            )
            .unwrap();
    });

    let mut config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    config.request_modes.insert(
        "plain_chat".to_string(),
        ProviderRequestProfile {
            backend: ProviderBackendKind::LmStudioNativeChat,
            stream: Some(false),
            reasoning: Some(ProviderReasoningLevel::Off),
            context_length: Some(8000),
            stats: Some(true),
            stateful: None,
        },
    );
    let provider = LmStudioProvider::new(config);
    let metadata = provider.request_metadata_for_mode("plain_chat");
    let output = provider
        .chat_messages_without_streaming_with_metadata(
            vec![
                ChatMessage::system("Answer briefly."),
                ChatMessage::user("hello"),
            ],
            &metadata,
        )
        .unwrap();

    server.join().unwrap();
    let request = receiver.recv().unwrap();
    assert!(request.starts_with("POST /api/v1/chat "));
    assert!(request.contains(r#""model":"loaded-model""#));
    assert!(request.contains(r#""input":"hello""#));
    assert!(request.contains(r#""system_prompt":"Answer briefly.""#));
    assert!(request.contains(r#""reasoning":"off""#));
    assert!(request.contains(r#""context_length":8000"#));

    assert_eq!(output.text, "Build passed.");
    assert_eq!(output.thinking.as_deref(), Some("Need answer."));
    let metrics = output.metrics.unwrap();
    assert_eq!(
        metrics.backend,
        Some(ProviderBackendKind::LmStudioNativeChat)
    );
    assert_eq!(metrics.reasoning, Some(ProviderReasoningLevel::Off));
    assert_eq!(metrics.context_length, Some(8000));
    assert_eq!(metrics.stats, Some(true));
    assert_eq!(metrics.provider_time_to_first_token_millis, Some(420));
    assert_eq!(metrics.provider_tokens_per_second_milli, Some(50_500));
    assert_eq!(metrics.reasoning_output_tokens, Some(2));
    assert_eq!(metrics.usage.unwrap().total_tokens, Some(15));
}

#[test]
fn live_streaming_chat_emits_reasoning_and_response_chunks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"reasoning_content":"Need greet."}}]}

"#,
        );
        thread::sleep(Duration::from_millis(5));
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Hello"}}]}

"#,
        );
        write_chunk(&mut stream, "data: [DONE]\n\n");
        stream.write_all(b"0\r\n\r\n").unwrap();
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let output =
        chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
            chunks.push(chunk);
        })
        .unwrap();

    server.join().unwrap();
    assert_eq!(output.text, "Hello");
    assert_eq!(output.thinking.as_deref(), Some("Need greet."));
    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("Need greet.".to_string()),
            ProviderStreamChunk::Text("Hello".to_string())
        ]
    );
}

#[test]
fn live_streaming_chat_rejects_incomplete_chunked_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Partial"}}]}

"#,
        );
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        timeout_millis: 1_000,
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let error = chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
        chunks.push(chunk);
    })
    .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
    assert!(error.message.contains("terminal chunk"));
    assert_eq!(
        chunks,
        vec![ProviderStreamChunk::Text("Partial".to_string())]
    );
}

#[test]
fn live_chat_reports_read_timeout_with_phase_without_external_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        thread::sleep(Duration::from_millis(40));
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        connect_timeout_millis: Some(1_000),
        read_timeout_millis: Some(10),
        request_timeout_millis: Some(1_000),
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let error = chat_lm_studio(&config, vec![ChatMessage::user("hello")]).unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::Network);
    assert!(error.message.contains("provider read timed out"));
}

#[test]
fn live_streaming_timeout_after_partial_chunk_returns_no_finished_output() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
            )
            .unwrap();
        write_chunk(
            &mut stream,
            r#"data: {"choices":[{"delta":{"content":"Partial"}}]}

"#,
        );
        thread::sleep(Duration::from_millis(40));
    });

    let config = ProviderConfig {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        stream: true,
        connect_timeout_millis: Some(1_000),
        read_timeout_millis: Some(10),
        request_timeout_millis: Some(1_000),
        ..ProviderConfig::lm_studio("loaded-model")
    };
    let mut chunks = Vec::new();
    let error = chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
        chunks.push(chunk);
    })
    .unwrap_err();

    server.join().unwrap();
    assert_eq!(error.kind, ProviderErrorKind::Network);
    assert!(error.message.contains("provider stream read timed out"));
    assert_eq!(
        chunks,
        vec![ProviderStreamChunk::Text("Partial".to_string())]
    );
}

fn write_chunk(stream: &mut std::net::TcpStream, body: &str) {
    write!(stream, "{:x}\r\n{}\r\n", body.len(), body).unwrap();
    stream.flush().unwrap();
}
