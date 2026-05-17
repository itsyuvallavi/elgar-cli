use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    event::ProviderOutput,
    provider::{
        config::ProviderConfig,
        http::{post_json, post_json_streaming, HttpEndpoint},
        types::{
            ChatMessage, ChatRequest, ChatResponse, ControllerProvider, ProviderError,
            ProviderErrorResponse, ProviderRequestMetadata, ProviderStreamChunk,
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

    for chunk in parse_chat_stream_chunks(payload)? {
        match chunk {
            ProviderStreamChunk::Reasoning(value) => thinking.push_str(&value),
            ProviderStreamChunk::Text(value) => text.push_str(&value),
        }
    }

    provider_output_from_stream_parts(text, thinking)
}

pub fn parse_chat_stream_chunks(payload: &str) -> Result<Vec<ProviderStreamChunk>, ProviderError> {
    let mut chunks = Vec::new();

    for line in payload.lines().map(str::trim) {
        chunks.extend(parse_chat_stream_line(line)?);
    }

    Ok(chunks)
}

pub fn parse_chat_stream_line(line: &str) -> Result<Vec<ProviderStreamChunk>, ProviderError> {
    if line.is_empty() || line.starts_with(':') {
        return Ok(Vec::new());
    }

    let Some(data) = line.strip_prefix("data:") else {
        return Ok(Vec::new());
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok(Vec::new());
    }

    let response: ChatStreamResponse = serde_json::from_str(data)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    let mut chunks = Vec::new();
    for choice in response.choices {
        if let Some(delta) = choice.delta {
            if let Some(reasoning) = non_empty(delta.reasoning) {
                chunks.push(ProviderStreamChunk::Reasoning(reasoning));
            }
            if let Some(thinking) = non_empty(delta.thinking) {
                chunks.push(ProviderStreamChunk::Reasoning(thinking));
            }
            if let Some(content) = non_empty(delta.content) {
                chunks.push(ProviderStreamChunk::Text(content));
            }
        }
    }

    Ok(chunks)
}

fn provider_output_from_stream_parts(
    text: String,
    thinking: String,
) -> Result<ProviderOutput, ProviderError> {
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

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.is_empty())
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

pub fn chat_lm_studio_streaming(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> Result<ProviderOutput, ProviderError> {
    if !config.stream {
        let output = chat_lm_studio(config, messages)?;
        emit_output_chunks(&output, on_chunk);
        return Ok(output);
    }

    let request = format_chat_request(config, messages)?;
    let body = serde_json::to_string(&request)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let timeout = Duration::from_millis(config.timeout_millis);
    let mut parts = StreamingOutputParts::default();
    let response = post_json_streaming(&endpoint, &body, timeout, &mut |body_chunk| {
        parts.push_body_chunk(body_chunk, on_chunk)
    })?;

    if response.status_code.is_success() {
        parts.finish(on_chunk)?;
        provider_output_from_stream_parts(parts.text, parts.thinking)
    } else {
        Err(parse_provider_error_json(
            Some(response.status_code.as_u16()),
            &response.body,
        ))
    }
}

fn emit_output_chunks(output: &ProviderOutput, on_chunk: &mut dyn FnMut(ProviderStreamChunk)) {
    if let Some(thinking) = output.thinking.as_ref() {
        on_chunk(ProviderStreamChunk::Reasoning(thinking.clone()));
    }
    on_chunk(ProviderStreamChunk::Text(output.text.clone()));
}

#[derive(Debug, Default)]
struct StreamingOutputParts {
    text: String,
    thinking: String,
    pending_line: String,
}

impl StreamingOutputParts {
    fn push_body_chunk(
        &mut self,
        body_chunk: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        self.pending_line.push_str(body_chunk);
        while let Some(newline) = self.pending_line.find('\n') {
            let mut line = self.pending_line[..newline].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            self.pending_line.drain(..=newline);
            self.push_line(&line, on_chunk)?;
        }
        Ok(())
    }

    fn finish(
        &mut self,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        if !self.pending_line.trim().is_empty() {
            let line = std::mem::take(&mut self.pending_line);
            self.push_line(line.trim_end_matches('\r'), on_chunk)?;
        }
        Ok(())
    }

    fn push_line(
        &mut self,
        line: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        for chunk in parse_chat_stream_line(line)? {
            match &chunk {
                ProviderStreamChunk::Reasoning(value) => self.thinking.push_str(value),
                ProviderStreamChunk::Text(value) => self.text.push_str(value),
            }
            on_chunk(chunk);
        }
        Ok(())
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

    fn chat_stream(
        &self,
        prompt: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<ProviderOutput, ProviderError> {
        chat_lm_studio_streaming(&self.config, vec![ChatMessage::user(prompt)], on_chunk)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    use super::{
        chat_lm_studio_streaming, format_chat_request, parse_chat_response_json,
        parse_chat_stream_chunks, parse_chat_stream_response, parse_provider_error_json,
        ChatMessage, ProviderConfig,
    };
    use crate::provider::{http::parse_http_response, ProviderErrorKind, ProviderStreamChunk};

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
        let error =
            chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
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

    fn write_chunk(stream: &mut std::net::TcpStream, body: &str) {
        write!(stream, "{:x}\r\n{}\r\n", body.len(), body).unwrap();
        stream.flush().unwrap();
    }
}
