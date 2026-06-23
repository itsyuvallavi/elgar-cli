//! High-level MCP client helpers.
//!
//! These helpers perform the standard MCP initialization and read-only list
//! calls over an HTTP transport.

use std::collections::BTreeMap;

use super::{
    config::McpHttpServerConfig,
    error::McpError,
    http::McpHttpClient,
    logging::{
        log_initialize_finished, log_resources_listed, log_tool_call_failed,
        log_tool_call_finished, log_tool_call_started, log_tools_listed, McpLogContext,
    },
    protocol::{
        initialize_request, initialized_notification, resources_list_request, tools_call_request,
        tools_list_request, InitializeResult, ResourcesListResult, ToolCallResult, ToolsListResult,
        MCP_PROTOCOL_VERSION,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpHttpDiscovery {
    pub initialize: InitializeResult,
    pub tools: Option<ToolsListResult>,
    pub resources: Option<ResourcesListResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpHttpToolCall {
    pub initialize: InitializeResult,
    pub tools: ToolsListResult,
    pub result: ToolCallResult,
}

pub fn discover_http_server(
    config: &McpHttpServerConfig,
    headers: BTreeMap<String, String>,
    client_version: &str,
    log_context: Option<McpLogContext>,
) -> Result<McpHttpDiscovery, McpError> {
    let mut client = McpHttpClient::new(
        config.url.clone(),
        headers,
        config.timeout_millis,
        MCP_PROTOCOL_VERSION,
    )?
    .with_log_context(log_context.clone());

    let initialize: InitializeResult =
        client.post_request(&initialize_request(1, "elgar", client_version))?;
    if let Some(context) = &log_context {
        log_initialize_finished(
            context,
            &initialize.server_info.name,
            &initialize.protocol_version,
        );
    }
    client.post_notification(&initialized_notification())?;

    let tools = if initialize.capabilities.tools.is_some() {
        let tools: ToolsListResult = client.post_request(&tools_list_request(2, None))?;
        if let Some(context) = &log_context {
            log_tools_listed(context, tools.tools.len());
        }
        Some(tools)
    } else {
        None
    };
    let resources = if initialize.capabilities.resources.is_some() {
        let resources: ResourcesListResult =
            client.post_request(&resources_list_request(3, None))?;
        if let Some(context) = &log_context {
            log_resources_listed(context, resources.resources.len());
        }
        Some(resources)
    } else {
        None
    };

    Ok(McpHttpDiscovery {
        initialize,
        tools,
        resources,
    })
}

pub fn call_http_tool(
    config: &McpHttpServerConfig,
    headers: BTreeMap<String, String>,
    client_version: &str,
    tool_name: &str,
    arguments: serde_json::Value,
    log_context: Option<McpLogContext>,
) -> Result<McpHttpToolCall, McpError> {
    let mut client = McpHttpClient::new(
        config.url.clone(),
        headers,
        config.timeout_millis,
        MCP_PROTOCOL_VERSION,
    )?
    .with_log_context(log_context.clone());

    let initialize: InitializeResult =
        client.post_request(&initialize_request(1, "elgar", client_version))?;
    if let Some(context) = &log_context {
        log_initialize_finished(
            context,
            &initialize.server_info.name,
            &initialize.protocol_version,
        );
    }
    client.post_notification(&initialized_notification())?;

    let tools: ToolsListResult = client.post_request(&tools_list_request(2, None))?;
    if let Some(context) = &log_context {
        log_tools_listed(context, tools.tools.len());
    }
    if !tools.tools.iter().any(|tool| tool.name == tool_name) {
        if let Some(context) = &log_context {
            log_tool_call_failed(context, tool_name, "unknown_tool");
        }
        return Err(McpError::Configuration(format!(
            "MCP tool `{tool_name}` is not listed by this server"
        )));
    }

    if let Some(context) = &log_context {
        log_tool_call_started(context, tool_name);
    }
    let result: ToolCallResult =
        match client.post_request(&tools_call_request(3, tool_name, arguments)) {
            Ok(result) => result,
            Err(error) => {
                if let Some(context) = &log_context {
                    log_tool_call_failed(context, tool_name, error_kind(&error));
                }
                return Err(error);
            }
        };
    if let Some(context) = &log_context {
        let is_error = result.is_error.unwrap_or(false);
        log_tool_call_finished(context, tool_name, result.content.len(), is_error);
    }

    Ok(McpHttpToolCall {
        initialize,
        tools,
        result,
    })
}

fn error_kind(error: &McpError) -> &'static str {
    match error {
        McpError::Configuration(_) => "configuration",
        McpError::Network(_) => "network",
        McpError::HttpStatus { .. } => "http_status",
        McpError::ResponseParse(_) => "response_parse",
        McpError::JsonRpc { .. } => "json_rpc",
        McpError::UnsupportedTransport(_) => "unsupported_transport",
    }
}
