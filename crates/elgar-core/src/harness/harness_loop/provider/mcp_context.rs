//! MCP tool catalog rendering for provider prompts.
//!
//! The harness exposes one generic `mcp_call` provider tool. This module gives
//! the model a bounded, live catalog of configured MCP server tools so it does
//! not need to guess server-specific tool names or argument shapes.

use crate::{
    mcp::{
        client::discover_http_server,
        config::{load_runtime_mcp_config, resolve_secret_sources, McpServerConfig},
        logging::McpLogContext,
        protocol::{McpTool, ToolsListResult},
    },
    session::Session,
};

const MAX_CATALOG_CHARS: usize = 6_000;
const MAX_DESCRIPTION_CHARS: usize = 240;
const MAX_SCHEMA_CHARS: usize = 1_200;
const MCP_CATALOG_HEADER: &str =
    "Active MCP tools (use `mcp_call`; arguments must match each tool schema):";

pub(in crate::harness::harness_loop) fn render_mcp_tool_catalog_for_prompt(
    session: &Session,
) -> Option<String> {
    let runtime = load_runtime_mcp_config(&session.project_root)
        .ok()
        .flatten()?;
    if runtime.config.servers.is_empty() {
        return None;
    }

    let mut output = String::from(MCP_CATALOG_HEADER);
    output.push('\n');

    for (server_id, server) in runtime.config.servers {
        match server {
            McpServerConfig::Http(config) => {
                let headers = match resolve_secret_sources(&config.headers) {
                    Ok(headers) => headers,
                    Err(error) => {
                        push_bounded(
                            &mut output,
                            format!("- server: {server_id}\n  unavailable: {error}\n"),
                        );
                        continue;
                    }
                };
                let context = McpLogContext {
                    project_root: session.project_root.clone(),
                    session_id: session.id.clone(),
                    turn_id: session.next_turn_id(),
                    server_id: server_id.clone(),
                    transport: "http".to_string(),
                };
                match discover_http_server(
                    &config,
                    headers,
                    env!("CARGO_PKG_VERSION"),
                    Some(context),
                ) {
                    Ok(discovery) => push_bounded(
                        &mut output,
                        render_http_server_tools(&server_id, discovery.tools.as_ref()),
                    ),
                    Err(error) => push_bounded(
                        &mut output,
                        format!("- server: {server_id}\n  unavailable: {error}\n"),
                    ),
                }
            }
            McpServerConfig::Stdio(_) => push_bounded(
                &mut output,
                format!("- server: {server_id}\n  unavailable: stdio MCP is not implemented yet\n"),
            ),
        }
    }

    Some(output)
}

fn render_http_server_tools(server_id: &str, tools: Option<&ToolsListResult>) -> String {
    let mut output = format!("- server: {server_id}\n  transport: http\n");
    let Some(tools) = tools else {
        output.push_str("  tools: none advertised\n");
        return output;
    };
    if tools.tools.is_empty() {
        output.push_str("  tools: none advertised\n");
        return output;
    }

    output.push_str("  tools:\n");
    for tool in &tools.tools {
        output.push_str(&render_tool(tool));
    }
    output
}

fn render_tool(tool: &McpTool) -> String {
    let description = tool
        .description
        .as_deref()
        .map(compact_description)
        .unwrap_or_default();
    let schema = serde_json::to_string(&tool.input_schema).unwrap_or_else(|_| "{}".to_string());
    let schema = truncate_chars(&schema, MAX_SCHEMA_CHARS);

    let mut output = format!("    - tool: {}\n", tool.name);
    if !description.is_empty() {
        output.push_str("      description: ");
        output.push_str(&description);
        output.push('\n');
    }
    output.push_str("      arguments_schema: ");
    output.push_str(&schema);
    output.push('\n');
    output
}

fn compact_description(value: &str) -> String {
    truncate_chars(
        &value.split_whitespace().collect::<Vec<_>>().join(" "),
        MAX_DESCRIPTION_CHARS,
    )
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn push_bounded(output: &mut String, value: String) {
    if output.chars().count() >= MAX_CATALOG_CHARS {
        return;
    }
    output.push_str(&value);
    if output.chars().count() > MAX_CATALOG_CHARS {
        *output = output.chars().take(MAX_CATALOG_CHARS).collect();
        output.push_str("\n[truncated: MCP catalog exceeded prompt budget]\n");
    }
}
