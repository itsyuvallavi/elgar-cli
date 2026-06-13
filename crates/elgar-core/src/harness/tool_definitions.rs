//! Provider tool definitions for primitive harness tools.
//!
//! The primitive registry owns which tools exist. This file only translates the
//! currently enabled primitives into compact OpenAI-compatible tool schemas for
//! provider calls that support tool use. Permission policy still decides which
//! tools may execute.

use serde_json::json;

use crate::provider::ChatToolDefinition;

use super::{PrimitiveToolId, PrimitiveToolRegistry};

/// Build provider tool definitions for enabled primitive tools.
pub(crate) fn provider_tool_definitions_for_registry(
    registry: &PrimitiveToolRegistry,
) -> Vec<ChatToolDefinition> {
    registry
        .tools()
        .iter()
        .filter(|tool| tool.enabled_in_stage)
        .filter_map(|tool| provider_tool_definition(tool.id))
        .collect()
}

fn provider_tool_definition(id: PrimitiveToolId) -> Option<ChatToolDefinition> {
    match id {
        PrimitiveToolId::Read => Some(ChatToolDefinition::function(
            "read",
            "Read bounded UTF-8 contents from one file path.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "File path relative to the launch folder, or an absolute path."
                    }
                }),
                &["path"],
            ),
        )),
        PrimitiveToolId::Ls => Some(ChatToolDefinition::function(
            "ls",
            "List one directory with bounded entries and counts.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Directory path relative to the launch folder, or an absolute path."
                    }
                }),
                &["path"],
            ),
        )),
        PrimitiveToolId::Find => Some(ChatToolDefinition::function(
            "find",
            "Find file and directory paths by name pattern under one directory.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Directory path to search. Use . for the launch folder."
                    },
                    "pattern": {
                        "type": "string",
                        "description": "Name pattern to match, such as README* or *config*."
                    }
                }),
                &["pattern"],
            ),
        )),
        PrimitiveToolId::Grep => Some(ChatToolDefinition::function(
            "grep",
            "Search text inside one bounded UTF-8 file or under one directory.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "File or directory path to search. Use . for the launch folder."
                    },
                    "query": {
                        "type": "string",
                        "description": "Text query to search for."
                    }
                }),
                &["query"],
            ),
        )),
        PrimitiveToolId::Bash => Some(ChatToolDefinition::function(
            "bash",
            "Request approval to run one shell command in the launch folder.",
            object_schema(
                json!({
                    "command": {
                        "type": "string",
                        "description": "Shell command to run after explicit user approval."
                    }
                }),
                &["command"],
            ),
        )),
        PrimitiveToolId::Write => Some(ChatToolDefinition::function(
            "write",
            "Request approval to create or overwrite one file.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "File path relative to the launch folder, or an absolute path."
                    },
                    "content": {
                        "type": "string",
                        "description": "Exact file content to write after explicit user approval."
                    }
                }),
                &["path", "content"],
            ),
        )),
        PrimitiveToolId::Edit => Some(ChatToolDefinition::function(
            "edit",
            "Request approval to replace exact text in one existing file.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "File path relative to the launch folder, or an absolute path."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Exact text that must appear exactly once before approval can edit the file."
                    },
                    "new_text": {
                        "type": "string",
                        "description": "Exact replacement text to write after explicit user approval."
                    }
                }),
                &["path", "old_text", "new_text"],
            ),
        )),
        PrimitiveToolId::McpCall => Some(ChatToolDefinition::function(
            "mcp_call",
            "Call one configured read-only MCP server tool and return bounded verified evidence.",
            object_schema(
                json!({
                    "server": {
                        "type": "string",
                        "description": "Configured MCP server id, such as context7."
                    },
                    "tool": {
                        "type": "string",
                        "description": "MCP tool name listed by the server, such as query-docs."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Exact JSON object arguments for the MCP tool."
                    }
                }),
                &["server", "tool", "arguments"],
            ),
        )),
    }
}

fn object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}
