//! Tool catalog for the internal Project Index MCP server.

use serde_json::json;

use crate::mcp::protocol::{McpTool, ToolsListResult};

/// Return the advertised internal Project Index tools.
pub fn project_index_tools() -> ToolsListResult {
    ToolsListResult {
        tools: vec![
            McpTool {
                name: "project_tree".to_string(),
                title: Some("Project tree".to_string()),
                description: Some(
                    "Return a bounded read-only tree summary for a launch-folder path.".to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative path inside the launch folder. Defaults to ."
                        }
                    },
                    "additionalProperties": false
                }),
            },
            McpTool {
                name: "project_find".to_string(),
                title: Some("Project find".to_string()),
                description: Some(
                    "Find bounded project paths by name pattern under a launch-folder path."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative directory inside the launch folder. Defaults to ."
                        },
                        "pattern": {
                            "type": "string",
                            "description": "Case-insensitive name pattern, such as page or *.tsx."
                        }
                    },
                    "required": ["pattern"],
                    "additionalProperties": false
                }),
            },
            McpTool {
                name: "project_read_summary".to_string(),
                title: Some("Project read summary".to_string()),
                description: Some(
                    "Read one bounded UTF-8 file inside the launch folder for verification."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "Relative file path inside the launch folder."
                        }
                    },
                    "required": ["path"],
                    "additionalProperties": false
                }),
            },
            McpTool {
                name: "project_status".to_string(),
                title: Some("Project status".to_string()),
                description: Some(
                    "Summarize current Elgar session state and pending approval status."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }),
            },
        ],
        next_cursor: None,
    }
}
