use std::{
    fmt, io,
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::event::ProviderOutput;

pub const LM_STUDIO_PROVIDER_NAME: &str = "lm-studio";
pub const LM_STUDIO_DEFAULT_BASE_URL: &str = "http://127.0.0.1:1234/v1";
pub const LM_STUDIO_DEFAULT_TIMEOUT_MILLIS: u64 = 30_000;

/// Data-only configuration for an LM Studio/OpenAI-compatible local provider.
///
/// This type is intentionally inert: it does not open sockets, perform health
/// checks, route requests, apply actions, or mutate project state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_provider_name")]
    pub provider: String,
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default = "default_timeout_millis")]
    pub timeout_millis: u64,
}

impl ProviderConfig {
    pub fn lm_studio(model: impl Into<String>) -> Self {
        Self {
            model: Some(model.into()),
            ..Self::default()
        }
    }

    pub fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            provider: default_provider_name(),
            base_url: default_base_url(),
            model: None,
            timeout_millis: default_timeout_millis(),
        }
    }
}

fn default_provider_name() -> String {
    LM_STUDIO_PROVIDER_NAME.to_string()
}

fn default_base_url() -> String {
    LM_STUDIO_DEFAULT_BASE_URL.to_string()
}

fn default_timeout_millis() -> u64 {
    LM_STUDIO_DEFAULT_TIMEOUT_MILLIS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self::new(ChatRole::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(ChatRole::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(ChatRole::Assistant, content)
    }

    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

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
        stream: false,
        temperature: None,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub choices: Vec<ChatChoice>,
    pub usage: Option<ChatUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: Option<u32>,
    pub message: Option<ChatMessage>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

pub fn parse_chat_response_json(payload: &str) -> Result<ProviderOutput, ProviderError> {
    let response: ChatResponse = serde_json::from_str(payload)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;

    let text = response
        .choices
        .iter()
        .filter_map(|choice| choice.message.as_ref())
        .find_map(|message| {
            let content = message.content.trim();
            (!content.is_empty()).then(|| content.to_string())
        })
        .ok_or_else(|| ProviderError::empty_response("provider response contained no text"))?;

    Ok(ProviderOutput::new(text))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorResponse {
    pub error: ProviderErrorBody,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorBody {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
    pub code: Option<String>,
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
/// This is intentionally not used by `Controller::turn` yet. Normal controller
/// behavior and tests remain no-network until a later issue wires provider mode.
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
        parse_chat_response_json(&response.body)
    } else {
        Err(parse_provider_error_json(
            Some(response.status_code.as_u16()),
            &response.body,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderErrorKind {
    Configuration,
    ResponseParse,
    Provider,
    EmptyResponse,
    Network,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    pub message: String,
    pub status_code: Option<u16>,
    pub code: Option<String>,
}

impl ProviderError {
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Configuration, message)
    }

    pub fn response_parse(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::ResponseParse, message)
    }

    pub fn provider(
        message: impl Into<String>,
        status_code: Option<u16>,
        code: Option<String>,
    ) -> Self {
        Self::new(ProviderErrorKind::Provider, message)
            .with_status(status_code)
            .with_code(code)
    }

    pub fn empty_response(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::EmptyResponse, message)
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ProviderErrorKind::Network, message)
    }

    fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status_code: None,
            code: None,
        }
    }

    fn with_status(mut self, status_code: Option<u16>) -> Self {
        self.status_code = status_code;
        self
    }

    fn with_code(mut self, code: Option<String>) -> Self {
        self.code = code;
        self
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status_code, self.code.as_deref()) {
            (Some(status), Some(code)) => write!(
                formatter,
                "{:?} provider error ({status}, {code}): {}",
                self.kind, self.message
            ),
            (Some(status), None) => {
                write!(
                    formatter,
                    "{:?} provider error ({status}): {}",
                    self.kind, self.message
                )
            }
            (None, Some(code)) => write!(
                formatter,
                "{:?} provider error ({code}): {}",
                self.kind, self.message
            ),
            (None, None) => write!(
                formatter,
                "{:?} provider error: {}",
                self.kind, self.message
            ),
        }
    }
}

impl std::error::Error for ProviderError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpEndpoint {
    host: String,
    port: u16,
    path: String,
}

impl HttpEndpoint {
    fn parse(url: &str) -> Result<Self, ProviderError> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            ProviderError::configuration("only http:// provider URLs are supported")
        })?;
        let (authority, path) = rest.split_once('/').ok_or_else(|| {
            ProviderError::configuration("provider URL must include a request path")
        })?;
        let (host, port) = parse_authority(authority)?;
        Ok(Self {
            host,
            port,
            path: format!("/{path}"),
        })
    }

    fn authority(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HttpStatusCode(u16);

impl HttpStatusCode {
    fn as_u16(self) -> u16 {
        self.0
    }

    fn is_success(self) -> bool {
        (200..300).contains(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status_code: HttpStatusCode,
    body: String,
}

fn parse_authority(authority: &str) -> Result<(String, u16), ProviderError> {
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => {
            let parsed_port = port
                .parse::<u16>()
                .map_err(|_| ProviderError::configuration("provider URL port is invalid"))?;
            (host, parsed_port)
        }
        None => (authority, 80),
    };

    if host.trim().is_empty() {
        return Err(ProviderError::configuration(
            "provider URL host must not be empty",
        ));
    }

    Ok((host.to_string(), port))
}

fn post_json(
    endpoint: &HttpEndpoint,
    body: &str,
    timeout: Duration,
) -> Result<HttpResponse, ProviderError> {
    let mut stream = TcpStream::connect(endpoint.authority())
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| ProviderError::network(error.to_string()))?;

    let request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        endpoint.path,
        endpoint.authority(),
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|error| ProviderError::network(error.to_string()))?;

    read_http_response(stream)
}

fn read_http_response(mut stream: TcpStream) -> Result<HttpResponse, ProviderError> {
    let mut bytes = Vec::new();
    stream
        .read_to_end(&mut bytes)
        .map_err(|error| match error.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                ProviderError::network("provider request timed out")
            }
            _ => ProviderError::network(error.to_string()),
        })?;
    let raw = String::from_utf8(bytes)
        .map_err(|error| ProviderError::response_parse(error.to_string()))?;
    parse_http_response(&raw)
}

fn parse_http_response(raw: &str) -> Result<HttpResponse, ProviderError> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing header/body split"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing status line"))?;
    let status_code = parse_status_code(status_line)?;

    Ok(HttpResponse {
        status_code,
        body: body.to_string(),
    })
}

fn parse_status_code(status_line: &str) -> Result<HttpStatusCode, ProviderError> {
    let mut parts = status_line.split_whitespace();
    let _version = parts
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing version"))?;
    let status = parts
        .next()
        .ok_or_else(|| ProviderError::response_parse("HTTP response missing status code"))?;
    let code = status
        .parse::<u16>()
        .map_err(|_| ProviderError::response_parse("HTTP response status code is invalid"))?;
    Ok(HttpStatusCode(code))
}

/// Deterministic provider stub for no-model controller tests.
///
/// This stub never performs network calls, filesystem writes, shell commands,
/// action transitions, or any other side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStub {
    pub provider: String,
    pub model: Option<String>,
}

impl ProviderStub {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: None,
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn ask(&self, prompt: &str) -> ProviderStubResponse {
        ProviderStubResponse {
            provider: self.provider.clone(),
            model: self.model.clone(),
            request_id: "stub-request-1".to_string(),
            output: ProviderOutput::new(format!("stub provider response to: {}", prompt.trim())),
        }
    }
}

impl Default for ProviderStub {
    fn default() -> Self {
        Self::new("stub-provider")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStubResponse {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: String,
    pub output: ProviderOutput,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        format_chat_request, parse_chat_response_json, parse_http_response,
        parse_provider_error_json, ChatMessage, HttpEndpoint, ProviderConfig, ProviderErrorKind,
        LM_STUDIO_DEFAULT_BASE_URL, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS, LM_STUDIO_PROVIDER_NAME,
    };

    #[test]
    fn provider_config_defaults_to_lm_studio_local_endpoint() {
        let config = ProviderConfig::default();

        assert_eq!(config.provider, LM_STUDIO_PROVIDER_NAME);
        assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(config.model, None);
        assert_eq!(config.timeout_millis, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS);
        assert_eq!(
            config.chat_completions_url(),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

    #[test]
    fn provider_config_deserializes_with_defaults() {
        let config: ProviderConfig = serde_json::from_value(json!({
            "model": "local-model"
        }))
        .unwrap();

        assert_eq!(config.provider, LM_STUDIO_PROVIDER_NAME);
        assert_eq!(config.base_url, LM_STUDIO_DEFAULT_BASE_URL);
        assert_eq!(config.model.as_deref(), Some("local-model"));
        assert_eq!(config.timeout_millis, LM_STUDIO_DEFAULT_TIMEOUT_MILLIS);
    }

    #[test]
    fn provider_config_trims_chat_url_slash() {
        let config = ProviderConfig {
            base_url: "http://127.0.0.1:1234/v1/".to_string(),
            ..ProviderConfig::lm_studio("loaded-model")
        };

        assert_eq!(
            config.chat_completions_url(),
            "http://127.0.0.1:1234/v1/chat/completions"
        );
    }

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
    fn live_chat_endpoint_parses_http_local_urls_only() {
        let endpoint = HttpEndpoint::parse("http://127.0.0.1:1234/v1/chat/completions").unwrap();

        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 1234);
        assert_eq!(endpoint.path, "/v1/chat/completions");
        assert_eq!(endpoint.authority(), "127.0.0.1:1234");

        let https = HttpEndpoint::parse("https://127.0.0.1:1234/v1/chat/completions").unwrap_err();
        assert_eq!(https.kind, ProviderErrorKind::Configuration);
        assert!(https.message.contains("http://"));
    }

    #[test]
    fn http_success_response_parses_as_provider_output() {
        let response = parse_http_response(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n\
            {\"choices\":[{\"message\":{\"role\":\"assistant\",\"content\":\"hello\"}}]}",
        )
        .unwrap();

        assert!(response.status_code.is_success());
        let output = parse_chat_response_json(&response.body).unwrap();
        assert_eq!(output.text, "hello");
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
    fn malformed_http_response_maps_to_parse_error() {
        let error = parse_http_response("HTTP/1.1 nope\r\n\r\n{}").unwrap_err();

        assert_eq!(error.kind, ProviderErrorKind::ResponseParse);
        assert!(error.message.contains("status code"));
    }
}
