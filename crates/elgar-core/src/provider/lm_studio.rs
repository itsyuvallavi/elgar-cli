use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        http::{post_json, HttpEndpoint},
        types::{
            ChatMessage, ChatRequest, ChatResponse, ControllerProvider, ProviderError,
            ProviderErrorResponse, ProviderRequestMetadata,
        },
    },
};

pub fn format_chat_request(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<ChatRequest, ProviderError> {
    let model = config
        .model
        .as_ref()
        .filter(|model| !model.trim().is_empty())
        .ok_or_else(|| ProviderError::configuration("provider model is required"))?;

    if messages.is_empty() {
        return Err(ProviderError::configuration(
            "at least one chat message is required",
        ));
    }

    Ok(ChatRequest {
        model: model.clone(),
        messages,
        stream: config.stream,
        temperature: None,
    })
}

pub fn parse_chat_response_json(payload: &str) -> Result<ProviderOutput, ProviderError> {
    let response: ChatResponse = serde_json::from_str(payload)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;

    let message = response
        .choices
        .iter()
        .filter_map(|choice| choice.message.as_ref())
        .find(|message| !message.content.trim().is_empty())
        .ok_or_else(|| ProviderError::empty_response("provider response contained no text"))?;

    let output = ProviderOutput::new(message.content.trim().to_string());

    Ok(match message.explicit_thinking() {
        Some(thinking) => output.with_thinking(thinking),
        None => output,
    })
}

pub fn parse_chat_stream_response(payload: &str) -> Result<ProviderOutput, ProviderError> {
    let mut text = String::new();
    let mut thinking = String::new();

    for line in payload.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }

        let response: ChatStreamResponse = serde_json::from_str(data)
            .map_err(|error| ProviderError::response_parse(error.to_string()))?;
        for choice in response.choices {
            if let Some(delta) = choice.delta {
                if let Some(content) = delta.content {
                    text.push_str(&content);
                }
                if let Some(reasoning) = delta.reasoning {
                    thinking.push_str(&reasoning);
                }
                if let Some(chunk) = delta.thinking {
                    thinking.push_str(&chunk);
                }
            }
        }
    }

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return Err(ProviderError::empty_response(
            "provider stream contained no text",
        ));
    }

    let output = ProviderOutput::new(trimmed_text.to_string());
    let trimmed_thinking = thinking.trim();
    Ok(if trimmed_thinking.is_empty() {
        output
    } else {
        output.with_thinking(trimmed_thinking.to_string())
    })
}

pub fn parse_provider_error_json(status_code: Option<u16>, payload: &str) -> ProviderError {
    match serde_json::from_str::<ProviderErrorResponse>(payload) {
        Ok(response) => {
            ProviderError::provider(response.error.message, status_code, response.error.code)
        }
        Err(error) => ProviderError::response_parse(error.to_string()).with_status(status_code),
    }
}

/// Explicit, opt-in live call for LM Studio/OpenAI-compatible local servers.
///
/// This is only used by explicit smoke/live-provider paths. Normal controller
/// behavior and tests remain no-network by default.
pub fn chat_lm_studio(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
) -> Result<ProviderOutput, ProviderError> {
    let request = format_chat_request(config, messages)?;
    let body = serde_json::to_string(&request)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let timeout = Duration::from_millis(config.timeout_millis);
    let response = post_json(&endpoint, &body, timeout)?;

    if response.status_code.is_success() {
        if request.stream {
            parse_chat_stream_response(&response.body)
        } else {
            parse_chat_response_json(&response.body)
        }
    } else {
        Err(parse_provider_error_json(
            Some(response.status_code.as_u16()),
            &response.body,
        ))
    }
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamResponse {
    #[serde(default)]
    choices: Vec<ChatStreamChoice>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamChoice {
    delta: Option<ChatStreamDelta>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
struct ChatStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default, alias = "reasoning_content")]
    reasoning: Option<String>,
    #[serde(default, alias = "thinking_content")]
    thinking: Option<String>,
}

impl ControllerProvider for LmStudioProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            self.config.provider.clone(),
            self.config.model.clone(),
            "lm-studio-request-1",
        )
    }

    fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio(&self.config, vec![ChatMessage::user(prompt)])
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        format_chat_request, parse_chat_response_json, parse_chat_stream_response,
        parse_provider_error_json, ChatMessage, ProviderConfig,
    };
    use crate::provider::{http::parse_http_response, ProviderErrorKind};

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
    fn request_formatting_requires_model_and_message() {
        let missing_model =
            format_chat_request(&ProviderConfig::default(), vec![ChatMessage::user("hello")])
                .unwrap_err();
        assert_eq!(missing_model.kind, ProviderErrorKind::Configuration);
        assert!(missing_model.message.contains("model"));

        let missing_message =
            format_chat_request(&ProviderConfig::lm_studio("loaded-model"), Vec::new())
                .unwrap_err();
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
                        "message": { "role": "assistant", "content": "  Suggested next step.  " },
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
}
