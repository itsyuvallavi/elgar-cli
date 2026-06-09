//! Provider tool definitions for primitive harness tools.
//!
//! The primitive registry owns which tools exist. This file only translates the
//! currently executable primitives into compact OpenAI-compatible tool schemas
//! for provider calls that support tool use.

use serde_json::json;

use crate::provider::ChatToolDefinition;

use super::{PrimitiveToolId, PrimitiveToolRegistry};

/// Build provider tool definitions for executable primitive tools.
pub(crate) fn provider_tool_definitions_for_registry(
    registry: &PrimitiveToolRegistry,
) -> Vec<ChatToolDefinition> {
    registry
        .tools()
        .iter()
        .filter(|tool| tool.enabled_in_stage && tool.executable_in_stage)
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
            "Search text inside bounded UTF-8 files under one directory.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Directory path to search. Use . for the launch folder."
                    },
                    "query": {
                        "type": "string",
                        "description": "Text query to search for."
                    }
                }),
                &["query"],
            ),
        )),
        PrimitiveToolId::Bash | PrimitiveToolId::Write | PrimitiveToolId::Edit => None,
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
