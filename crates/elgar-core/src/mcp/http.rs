//! HTTP transport for MCP JSON-RPC messages.
//!
//! This transport is separate from the provider HTTP client because MCP needs
//! remote HTTPS support while the provider client is intentionally localhost-only.

use std::{collections::BTreeMap, time::Duration};

use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use super::{
    error::McpError,
    logging::{
        log_http_request_failed, log_http_request_finished, log_http_request_started,
        McpLogContext, McpLogTimer,
    },
    protocol::{JsonRpcErrorResponse, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse},
};

const DEFAULT_TIMEOUT_MILLIS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpHttpClient {
    endpoint: String,
    headers: BTreeMap<String, String>,
    timeout_millis: u64,
    session_id: Option<String>,
    protocol_version: String,
    log_context: Option<McpLogContext>,
}

impl McpHttpClient {
    pub fn new(
        endpoint: impl Into<String>,
        headers: BTreeMap<String, String>,
        timeout_millis: Option<u64>,
        protocol_version: impl Into<String>,
    ) -> Result<Self, McpError> {
        let endpoint = endpoint.into();
        let trimmed = endpoint.trim();
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return Err(McpError::Configuration(
                "HTTP MCP endpoint must start with http:// or https://".to_string(),
            ));
        }

        Ok(Self {
            endpoint: trimmed.to_string(),
            headers,
            timeout_millis: timeout_millis.unwrap_or(DEFAULT_TIMEOUT_MILLIS).max(1),
            session_id: None,
            protocol_version: protocol_version.into(),
            log_context: None,
        })
    }

    pub fn with_log_context(mut self, context: Option<McpLogContext>) -> Self {
        self.log_context = context;
        self
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn post_request<T, R>(&mut self, request: &JsonRpcRequest<T>) -> Result<R, McpError>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        let body = serde_json::to_string(request)
            .map_err(|error| McpError::ResponseParse(error.to_string()))?;
        let response = self.post_json_body(&request.method, &body)?;
        parse_json_rpc_response(request.id, &response)
    }

    pub fn post_notification<T>(
        &mut self,
        notification: &JsonRpcNotification<T>,
    ) -> Result<(), McpError>
    where
        T: Serialize,
    {
        let body = serde_json::to_string(notification)
            .map_err(|error| McpError::ResponseParse(error.to_string()))?;
        let _response = self.post_json_body(&notification.method, &body)?;
        Ok(())
    }

    fn post_json_body(&mut self, method: &str, body: &str) -> Result<String, McpError> {
        if let Some(context) = &self.log_context {
            log_http_request_started(context, method);
        }
        let timer = McpLogTimer::start();
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(self.timeout_millis))
            .build();
        let mut request = agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .set("Accept", "application/json, text/event-stream")
            .set("MCP-Protocol-Version", &self.protocol_version);

        for (name, value) in &self.headers {
            request = request.set(name, value);
        }
        if let Some(session_id) = self.session_id.as_deref() {
            request = request.set("Mcp-Session-Id", session_id);
        }

        match request.send_string(body) {
            Ok(response) => {
                let status_code = response.status();
                if let Some(session_id) = response.header("Mcp-Session-Id") {
                    self.session_id = Some(session_id.to_string());
                }
                if let Some(context) = &self.log_context {
                    log_http_request_finished(
                        context,
                        method,
                        status_code,
                        timer.elapsed_ms(),
                        self.session_id.is_some(),
                    );
                }
                response
                    .into_string()
                    .map_err(|error| McpError::ResponseParse(error.to_string()))
            }
            Err(ureq::Error::Status(status, response)) => {
                let body = response.into_string().unwrap_or_default();
                if let Some(context) = &self.log_context {
                    log_http_request_failed(
                        context,
                        method,
                        timer.elapsed_ms(),
                        "http_status",
                        Some(status),
                    );
                }
                Err(McpError::HttpStatus { status, body })
            }
            Err(error) => {
                if let Some(context) = &self.log_context {
                    log_http_request_failed(context, method, timer.elapsed_ms(), "network", None);
                }
                Err(McpError::Network(error.to_string()))
            }
        }
    }
}

fn parse_json_rpc_response<R>(expected_id: u64, raw_body: &str) -> Result<R, McpError>
where
    R: DeserializeOwned,
{
    let body = extract_json_rpc_body(raw_body)?;
    let value: Value =
        serde_json::from_str(&body).map_err(|error| McpError::ResponseParse(error.to_string()))?;
    if value.get("error").is_some() {
        let error: JsonRpcErrorResponse = serde_json::from_value(value)
            .map_err(|error| McpError::ResponseParse(error.to_string()))?;
        return Err(McpError::JsonRpc {
            code: error.error.code,
            message: error.error.message,
        });
    }

    let response: JsonRpcResponse<R> = serde_json::from_value(value)
        .map_err(|error| McpError::ResponseParse(error.to_string()))?;
    if response.id != expected_id {
        return Err(McpError::ResponseParse(format!(
            "MCP response id {} did not match request id {expected_id}",
            response.id
        )));
    }
    Ok(response.result)
}

fn extract_json_rpc_body(raw_body: &str) -> Result<String, McpError> {
    let trimmed = raw_body.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed.to_string());
    }

    let mut data_lines = Vec::new();
    for line in trimmed.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if !data.is_empty() {
            data_lines.push(data);
        }
    }
    if data_lines.is_empty() {
        return Err(McpError::ResponseParse(
            "MCP response did not contain JSON or SSE data".to_string(),
        ));
    }

    Ok(data_lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{extract_json_rpc_body, parse_json_rpc_response};
    use crate::mcp::protocol::ToolsListResult;

    #[test]
    fn parses_plain_json_rpc_response() {
        let result: ToolsListResult =
            parse_json_rpc_response(1, r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#)
                .expect("plain JSON response should parse");

        assert!(result.tools.is_empty());
    }

    #[test]
    fn parses_sse_json_rpc_response() {
        let body = extract_json_rpc_body(
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}\n\n",
        )
        .expect("SSE body should parse");
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("body is JSON");

        assert_eq!(
            parsed,
            json!({"jsonrpc":"2.0","id":1,"result":{"tools":[]}})
        );
    }

    #[test]
    fn rejects_mismatched_response_id() {
        let error = parse_json_rpc_response::<ToolsListResult>(
            2,
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#,
        )
        .expect_err("mismatched id should fail");

        assert!(error.to_string().contains("did not match"));
    }
}
