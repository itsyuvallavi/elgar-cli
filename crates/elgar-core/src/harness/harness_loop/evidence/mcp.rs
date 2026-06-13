//! MCP evidence execution for the harness loop.
//!
//! This module turns a validated `mcp_call` model tool request into bounded
//! verified evidence.

use serde_json::Value;

use crate::{
    harness::{
        harness_loop::state::types::Evidence, ModelChoiceTurnError, ValidatedStructuredRequest,
    },
    mcp::{
        client::call_http_tool,
        config::{load_runtime_mcp_config, resolve_secret_sources, McpServerConfig},
        logging::McpLogContext,
        protocol::ToolCallResult,
    },
    session::Session,
};

const MAX_MCP_RESULT_CHARS: usize = 6_000;

pub(in crate::harness::harness_loop) fn execute_mcp_call_request(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<Evidence, ModelChoiceTurnError> {
    let arguments = request
        .arguments
        .as_ref()
        .ok_or_else(|| ModelChoiceTurnError::ProjectContext("mcp_call missing arguments".into()))?;
    let server_id = required_string(arguments, "server")?;
    let tool_name = required_string(arguments, "tool")?;
    let tool_arguments = arguments
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| Value::Object(Default::default()));
    if !tool_arguments.is_object() {
        return Err(ModelChoiceTurnError::ProjectContext(
            "mcp_call arguments.arguments must be an object".into(),
        ));
    }

    let runtime = load_runtime_mcp_config(&session.project_root)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?
        .ok_or_else(|| ModelChoiceTurnError::ProjectContext("MCP config not found".into()))?;
    let server = runtime.config.servers.get(server_id).ok_or_else(|| {
        ModelChoiceTurnError::ProjectContext(format!("MCP server `{server_id}` not found"))
    })?;

    match server {
        McpServerConfig::Http(config) => {
            let headers = resolve_secret_sources(&config.headers)
                .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let context = McpLogContext {
                project_root: session.project_root.clone(),
                session_id: session.id.clone(),
                turn_id: session.next_turn_id(),
                server_id: server_id.to_string(),
                transport: "http".to_string(),
            };
            let result = call_http_tool(
                config,
                headers,
                env!("CARGO_PKG_VERSION"),
                tool_name,
                tool_arguments,
                Some(context),
            )
            .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let body = render_mcp_evidence(server_id, tool_name, &result.result);
            Ok(Evidence {
                label: format!("mcp:{server_id}:{tool_name}"),
                bytes: body.len(),
                truncated: body.chars().count() >= MAX_MCP_RESULT_CHARS,
                body,
            })
        }
        McpServerConfig::Stdio(_) => Err(ModelChoiceTurnError::ProjectContext(format!(
            "MCP server `{server_id}` uses stdio, which is not enabled for harness calls yet"
        ))),
    }
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, ModelChoiceTurnError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelChoiceTurnError::ProjectContext(format!("mcp_call missing {key}")))
}

fn render_mcp_evidence(server_id: &str, tool_name: &str, result: &ToolCallResult) -> String {
    let is_error = result.is_error.unwrap_or(false);
    let mut body = format!(
        "VERIFIED_MCP_TOOL_RESULT\nserver: {server_id}\ntool: {tool_name}\nis_error: {is_error}\ncontent:\n"
    );
    let rendered_content = result
        .content
        .iter()
        .map(render_content_block)
        .collect::<Vec<_>>()
        .join("\n");
    body.push_str(&bounded_text(&rendered_content));
    body
}

fn render_content_block(value: &Value) -> String {
    if let Some(text) = value.get("text").and_then(Value::as_str) {
        return text.to_string();
    }
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn bounded_text(value: &str) -> String {
    if value.chars().count() <= MAX_MCP_RESULT_CHARS {
        return value.to_string();
    }
    let mut clipped = value.chars().take(MAX_MCP_RESULT_CHARS).collect::<String>();
    clipped.push_str("\n[truncated MCP result]");
    clipped
}
