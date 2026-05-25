use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    event::{ProviderMetrics, ProviderOutput},
    provider::{
        config::ProviderConfig,
        http::{post_json, post_json_streaming, HttpEndpoint, HttpTimeouts},
        lm_studio_format::elgar_controller_messages_for_config,
        lm_studio_parse::provider_output_from_stream_parts,
        types::{
            ChatMessage, ChatRequest, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata, ProviderStreamChunk,
        },
    },
};

#[cfg(test)]
pub(crate) use super::lm_studio_format::elgar_controller_messages;
pub use super::lm_studio_format::{
    format_chat_request, format_chat_request_body, format_chat_request_body_with_tools,
    format_chat_request_with_tools,
};
pub use super::lm_studio_parse::{
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
    chat_lm_studio_with_request_id(config, messages, &request_id)
}

fn chat_lm_studio_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    let (request, body) = format_chat_request_body(config, messages)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len());
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let response = post_json(&endpoint, &body, http_timeouts(config))?;

    if response.status_code.is_success() {
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        if request.stream {
            let output = parse_chat_stream_response(&response.body)?;
            Ok(output.with_metrics(metrics))
        } else {
            parse_chat_response_json_with_metrics(&response.body, Some(metrics))
        }
    } else {
        Err(parse_provider_error_json(
            Some(response.status_code.as_u16()),
            &response.body,
        ))
    }
}

fn chat_lm_studio_with_tools_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    tools: Vec<ChatToolDefinition>,
) -> Result<ProviderOutput, ProviderError> {
    let started = Instant::now();
    let mut config = config.clone();
    config.stream = false;
    let (request, body) = format_chat_request_body_with_tools(&config, messages, tools)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len());
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let response = post_json(&endpoint, &body, http_timeouts(&config))?;

    if response.status_code.is_success() {
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        parse_chat_response_json_with_metrics(&response.body, Some(metrics))
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
    let request_id = next_lm_studio_request_id();
    chat_lm_studio_streaming_with_request_id(config, messages, &request_id, on_chunk)
}

fn chat_lm_studio_streaming_with_request_id(
    config: &ProviderConfig,
    messages: Vec<ChatMessage>,
    request_id: &str,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) -> Result<ProviderOutput, ProviderError> {
    if !config.stream {
        let output = chat_lm_studio_with_request_id(config, messages, request_id)?;
        emit_output_chunks(&output, on_chunk);
        return Ok(output);
    }

    let started = Instant::now();
    let (request, body) = format_chat_request_body(config, messages)?;
    let mut metrics = metrics_for_request(request_id, &request, body.len());
    let endpoint = HttpEndpoint::parse(&config.chat_completions_url())?;
    let mut parts = StreamingOutputParts::default();
    let response =
        post_json_streaming(&endpoint, &body, http_timeouts(config), &mut |body_chunk| {
            parts.push_body_chunk(body_chunk, &mut |chunk| {
                if metrics.first_chunk_latency_millis.is_none() {
                    metrics.first_chunk_latency_millis = Some(duration_millis(started.elapsed()));
                }
                on_chunk(chunk);
            })
        })?;

    if response.status_code.is_success() {
        parts.finish(&mut |chunk| {
            if metrics.first_chunk_latency_millis.is_none() {
                metrics.first_chunk_latency_millis = Some(duration_millis(started.elapsed()));
            }
            on_chunk(chunk);
        })?;
        metrics.total_duration_millis = Some(duration_millis(started.elapsed()));
        let output = provider_output_from_stream_parts(parts.text, parts.thinking)?;
        Ok(output.with_metrics(metrics))
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

fn http_timeouts(config: &ProviderConfig) -> HttpTimeouts {
    HttpTimeouts::from_millis(
        config.connect_timeout_millis(),
        config.read_timeout_millis(),
        config.write_timeout_millis(),
        config.request_timeout_millis(),
    )
}

fn metrics_for_request(
    request_id: &str,
    request: &ChatRequest,
    body_len: usize,
) -> ProviderMetrics {
    ProviderMetrics::new(
        request_id,
        Some(request.model.clone()),
        request.stream,
        request.messages.len(),
        body_len,
    )
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
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

impl ControllerProvider for LmStudioProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            self.config.provider.clone(),
            self.config.model.clone(),
            next_lm_studio_request_id(),
        )
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
        chat_lm_studio_with_request_id(
            &self.config,
            elgar_controller_messages_for_config(&self.config, prompt),
            &metadata.request_id,
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
        )
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
mod tests {
    use serde_json::json;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::Duration,
    };

    use super::{
        chat_lm_studio, chat_lm_studio_streaming, elgar_controller_messages,
        elgar_controller_messages_for_config, format_chat_request, format_chat_request_body,
        format_chat_request_with_tools, parse_chat_response_json,
        parse_chat_response_json_with_metrics, parse_chat_stream_chunks,
        parse_chat_stream_response, parse_provider_error_json, ChatMessage, LmStudioProvider,
        ProviderConfig,
    };
    use crate::event::ProviderMetrics;
    use crate::model_runtime::{elgar_model_tool_definitions, ModelToolName, RawModelToolName};
    use crate::provider::{
        http::parse_http_response, ControllerProvider, ProviderErrorKind, ProviderStreamChunk,
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
    fn formats_tool_enabled_openai_compatible_chat_request() {
        let config = ProviderConfig::lm_studio("loaded-model");

        let request = format_chat_request_with_tools(
            &config,
            vec![ChatMessage::user("create hello.py")],
            elgar_model_tool_definitions(),
        )
        .unwrap();
        let value = serde_json::to_value(&request).unwrap();

        assert_eq!(value["tool_choice"], "auto");
        assert_eq!(value["tools"].as_array().unwrap().len(), 8);
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "ask_guidance");
        assert_eq!(value["tools"][1]["function"]["name"], "create_file");
        assert_eq!(
            value["tools"][1]["function"]["parameters"]["required"],
            json!(["target_path", "contents"])
        );
        assert_eq!(value["tools"][7]["function"]["name"], "shell_command");
        assert!(value["tools"][7]["function"]["parameters"]["properties"]
            .get("command")
            .is_some());
        assert!(value["tools"][7]["function"]["parameters"]["properties"]
            .get("cwd")
            .is_some());
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
            format_chat_request_body(&config, elgar_controller_messages("what can you do?"))
                .unwrap();
        let metrics = super::metrics_for_request("request-compact", &request, body.len());

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
        let metrics = super::metrics_for_request("request-1", &request, body.len());

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
        assert!(output.tool_calls.is_empty());
        assert_eq!(output.thinking, None);
    }

    #[test]
    fn parses_openai_compatible_tool_call_as_raw_draft() {
        let output = parse_chat_response_json(
            r#"{
                "id": "chatcmpl-local",
                "model": "loaded-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": null,
                            "tool_calls": [
                                {
                                    "id": "call_1",
                                    "type": "function",
                                    "function": {
                                        "name": "create_file",
                                        "arguments": "{\"target_path\":\"demo.txt\",\"contents\":\"hi\"}"
                                    }
                                }
                            ]
                        },
                        "finish_reason": "tool_calls"
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(output.text, "");
        assert_eq!(output.tool_calls.len(), 1);
        assert_eq!(output.tool_calls[0].id, "call_1");
        assert_eq!(
            output.tool_calls[0].name,
            RawModelToolName::Known(ModelToolName::CreateFile)
        );
        assert_eq!(
            output.tool_calls[0].arguments,
            json!({ "target_path": "demo.txt", "contents": "hi" })
        );
    }

    #[test]
    fn parses_content_and_tool_call_without_losing_either() {
        let output = parse_chat_response_json(
            r#"{
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "  I can draft that.  ",
                            "tool_calls": [
                                {
                                    "id": "call_dir",
                                    "type": "function",
                                    "function": {
                                        "name": "create_directory",
                                        "arguments": "{\"target_path\":\"notes\"}"
                                    }
                                }
                            ]
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(output.text, "I can draft that.");
        assert_eq!(output.tool_calls.len(), 1);
        assert_eq!(
            output.tool_calls[0].name,
            RawModelToolName::Known(ModelToolName::CreateDirectory)
        );
        assert_eq!(
            output.tool_calls[0].arguments,
            json!({ "target_path": "notes" })
        );
    }

    #[test]
    fn parses_unknown_tool_name_as_raw_unknown_draft() {
        let output = parse_chat_response_json(
            r#"{
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "",
                            "tool_calls": [
                                {
                                    "id": "call_unknown",
                                    "type": "function",
                                    "function": {
                                        "name": "surprise_tool",
                                        "arguments": "{\"value\":42}"
                                    }
                                }
                            ]
                        }
                    }
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(output.tool_calls.len(), 1);
        assert_eq!(
            output.tool_calls[0].name,
            RawModelToolName::Unknown("surprise_tool".to_string())
        );
        assert_eq!(output.tool_calls[0].arguments, json!({ "value": 42 }));
    }

    #[test]
    fn malformed_tool_call_arguments_fail_safely() {
        let error = parse_chat_response_json(
            r#"{
                "choices": [
                    {
                        "message": {
                            "role": "assistant",
                            "content": "",
                            "tool_calls": [
                                {
                                    "id": "call_bad_args",
                                    "type": "function",
                                    "function": {
                                        "name": "create_file",
                                        "arguments": "{not json"
                                    }
                                }
                            ]
                        }
                    }
                ]
            }"#,
        )
        .unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
        assert!(error.message.contains("call_bad_args"));
        assert!(error.message.contains("arguments"));
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
        let error =
            chat_lm_studio_streaming(&config, vec![ChatMessage::user("hello")], &mut |chunk| {
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
}
