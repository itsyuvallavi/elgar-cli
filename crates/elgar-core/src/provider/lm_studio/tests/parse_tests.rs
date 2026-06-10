//! LM Studio response and streaming parse tests.

use super::super::{
    parse_chat_response_json, parse_chat_stream_chunks, parse_chat_stream_response,
    parse_provider_error_json,
};
use crate::provider::{http::parse_http_response, ProviderErrorKind, ProviderStreamChunk};

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
