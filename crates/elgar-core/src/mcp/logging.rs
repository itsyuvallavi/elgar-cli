//! System-log helpers for MCP diagnostics.
//!
//! MCP logs go to the system JSONL log because discovery/connectivity is
//! runtime diagnostics, not conversation history.

use std::{path::PathBuf, time::Instant};

use serde_json::{json, Value};

use crate::logs::system::{append_log_event, LogInput, LogPhase};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpLogContext {
    pub project_root: PathBuf,
    pub session_id: String,
    pub turn_id: u64,
    pub server_id: String,
    pub transport: String,
}

#[derive(Debug, Clone)]
pub struct McpLogTimer {
    started: Instant,
}

impl McpLogTimer {
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

pub fn log_config_loaded(
    context: &McpLogContext,
    source_path: &str,
    server_count: usize,
    selected_server_id: &str,
) {
    log_mcp_event(
        context,
        "mcp_config_loaded",
        None,
        json!({
            "source_path": source_path,
            "server_count": server_count,
            "selected_server_id": selected_server_id
        }),
    );
}

pub fn log_http_request_started(context: &McpLogContext, method: &str) {
    log_mcp_event(
        context,
        "mcp_http_request_started",
        None,
        json!({
            "method": method
        }),
    );
}

pub fn log_http_request_finished(
    context: &McpLogContext,
    method: &str,
    status_code: u16,
    duration_ms: u64,
    mcp_session_id_present: bool,
) {
    log_mcp_event(
        context,
        "mcp_http_request_finished",
        Some(duration_ms),
        json!({
            "method": method,
            "status_code": status_code,
            "mcp_session_id_present": mcp_session_id_present
        }),
    );
}

pub fn log_http_request_failed(
    context: &McpLogContext,
    method: &str,
    duration_ms: u64,
    error_kind: &str,
    status_code: Option<u16>,
) {
    log_mcp_event(
        context,
        "mcp_http_request_failed",
        Some(duration_ms),
        json!({
            "method": method,
            "error_kind": error_kind,
            "status_code": status_code
        }),
    );
}

pub fn log_initialize_finished(context: &McpLogContext, server_name: &str, protocol_version: &str) {
    log_mcp_event(
        context,
        "mcp_initialize_finished",
        None,
        json!({
            "server_name": server_name,
            "protocol_version": protocol_version
        }),
    );
}

pub fn log_tools_listed(context: &McpLogContext, tool_count: usize) {
    log_mcp_event(
        context,
        "mcp_tools_listed",
        None,
        json!({
            "tool_count": tool_count
        }),
    );
}

pub fn log_resources_listed(context: &McpLogContext, resource_count: usize) {
    log_mcp_event(
        context,
        "mcp_resources_listed",
        None,
        json!({
            "resource_count": resource_count
        }),
    );
}

pub fn log_tool_call_started(context: &McpLogContext, tool_name: &str) {
    log_mcp_event(
        context,
        "mcp_tool_call_started",
        None,
        json!({
            "tool_name": tool_name
        }),
    );
}

pub fn log_tool_call_finished(
    context: &McpLogContext,
    tool_name: &str,
    content_count: usize,
    is_error: bool,
) {
    log_mcp_event(
        context,
        "mcp_tool_call_finished",
        None,
        json!({
            "tool_name": tool_name,
            "content_count": content_count,
            "is_error": is_error
        }),
    );
}

pub fn log_tool_call_failed(context: &McpLogContext, tool_name: &str, error_kind: &str) {
    log_mcp_event(
        context,
        "mcp_tool_call_failed",
        None,
        json!({
            "tool_name": tool_name,
            "error_kind": error_kind
        }),
    );
}

fn log_mcp_event(
    context: &McpLogContext,
    summary: &'static str,
    duration_ms: Option<u64>,
    metadata: Value,
) {
    let mut metadata = metadata;
    add_common_metadata(context, &mut metadata);
    let input = LogInput::new(
        context.turn_id,
        LogPhase::Runtime,
        file!(),
        "mcp_diagnostic",
        summary,
    )
    .with_metadata(metadata);
    let input = if let Some(duration_ms) = duration_ms {
        input.with_duration_ms(duration_ms)
    } else {
        input
    };

    let _ = append_log_event(&context.project_root, &context.session_id, input);
}

fn add_common_metadata(context: &McpLogContext, metadata: &mut Value) {
    let Some(object) = metadata.as_object_mut() else {
        return;
    };
    object.insert("server_id".to_string(), json!(context.server_id));
    object.insert("transport".to_string(), json!(context.transport));
}
