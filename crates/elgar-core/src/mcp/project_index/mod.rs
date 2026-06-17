//! Internal read-only Project Index MCP tools.
//!
//! These tools expose bounded project inspection through the same `mcp_call`
//! path as remote MCP servers. They never execute side effects.

mod catalog;

#[cfg(test)]
mod tests;

use std::path::{Component, Path};

use serde_json::{json, Value};

use crate::{
    event::Event,
    harness::{
        collect_directory_summary, collect_find_matches, collect_project_file, DirectoryOptions,
        FindOptions, ProjectFileOptions,
    },
    mcp::protocol::ToolCallResult,
    session::Session,
};

pub use catalog::project_index_tools;

const DEFAULT_PATH: &str = ".";
const MAX_PROJECT_TREE_DEPTH: usize = 3;
const MAX_PROJECT_TREE_ENTRIES: usize = 240;
const MAX_PROJECT_READ_BYTES: usize = 8 * 1024;

/// Execute one internal Project Index tool call.
pub fn call_project_index_tool(
    session: &Session,
    tool_name: &str,
    arguments: &Value,
) -> ToolCallResult {
    match tool_name {
        "project_tree" => project_tree(session, arguments),
        "project_find" => project_find(session, arguments),
        "project_read_summary" => project_read_summary(session, arguments),
        "project_status" => project_status(session),
        _ => error_result(format!("unknown project-index tool `{tool_name}`")),
    }
}

fn project_tree(session: &Session, arguments: &Value) -> ToolCallResult {
    let path = match safe_relative_path_arg(arguments, "path", Some(DEFAULT_PATH)) {
        Ok(path) => path,
        Err(error) => return error_result(error),
    };
    let mut options = DirectoryOptions::default();
    options.max_depth = MAX_PROJECT_TREE_DEPTH;
    options.max_entries = MAX_PROJECT_TREE_ENTRIES;

    match collect_directory_summary(&session.cwd, &path, options) {
        Ok(snapshot) => text_result(format!(
            "PROJECT_INDEX_TREE\n{}",
            snapshot.render_for_model()
        )),
        Err(error) => error_result(error.to_string()),
    }
}

fn project_find(session: &Session, arguments: &Value) -> ToolCallResult {
    let path = match safe_relative_path_arg(arguments, "path", Some(DEFAULT_PATH)) {
        Ok(path) => path,
        Err(error) => return error_result(error),
    };
    let pattern = match string_arg(arguments, "pattern", None) {
        Ok(pattern) => pattern,
        Err(error) => return error_result(error),
    };

    match collect_find_matches(&session.cwd, &path, &pattern, FindOptions::default()) {
        Ok(snapshot) => text_result(format!(
            "PROJECT_INDEX_FIND\n{}",
            snapshot.render_for_model()
        )),
        Err(error) => error_result(error.to_string()),
    }
}

fn project_read_summary(session: &Session, arguments: &Value) -> ToolCallResult {
    let path = match safe_relative_path_arg(arguments, "path", None) {
        Ok(path) => path,
        Err(error) => return error_result(error),
    };
    let options = ProjectFileOptions {
        max_bytes: MAX_PROJECT_READ_BYTES,
    };

    match collect_project_file(&session.cwd, &path, options) {
        Ok(snapshot) => text_result(format!(
            "PROJECT_INDEX_READ_SUMMARY\n{}",
            snapshot.render_for_model()
        )),
        Err(error) => error_result(error.to_string()),
    }
}

fn project_status(session: &Session) -> ToolCallResult {
    let mut user_messages = 0usize;
    let mut assistant_messages = 0usize;
    let mut provider_started = 0usize;
    let mut provider_finished = 0usize;
    let mut errors = 0usize;

    for event in session.events() {
        match event {
            Event::UserMessage(_) => user_messages += 1,
            Event::AssistantMessage(_) => assistant_messages += 1,
            Event::ProviderStarted(_) => provider_started += 1,
            Event::ProviderFinished(_) => provider_finished += 1,
            Event::ProviderStreamChunk(_) => {}
            Event::Error(_) => errors += 1,
        }
    }

    let approval = session.pending_approval().map(|approval| {
        json!({
            "id": approval.id,
            "tool": approval.tool,
            "status": approval.status.as_str(),
            "steps": approval.steps.len()
        })
    });

    text_result(format!(
        "PROJECT_INDEX_STATUS\n{}",
        serde_json::to_string_pretty(&json!({
            "session_id": session.id,
            "cwd": session.cwd.display().to_string(),
            "events": {
                "user_messages": user_messages,
                "assistant_messages": assistant_messages,
                "provider_started": provider_started,
                "provider_finished": provider_finished,
                "errors": errors
            },
            "pending_approval": approval
        }))
        .unwrap_or_else(|_| "{}".to_string())
    ))
}

fn safe_relative_path_arg(
    arguments: &Value,
    key: &str,
    default: Option<&str>,
) -> Result<String, String> {
    let value = string_arg(arguments, key, default)?;
    let path = Path::new(&value);
    if path.is_absolute() {
        return Err(format!("`{key}` must be relative to the launch folder"));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(format!("`{key}` cannot contain parent-directory segments"));
    }
    Ok(value)
}

fn string_arg(arguments: &Value, key: &str, default: Option<&str>) -> Result<String, String> {
    match arguments.get(key).and_then(Value::as_str).map(str::trim) {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => default
            .map(str::to_string)
            .ok_or_else(|| format!("`{key}` is required")),
    }
}

fn text_result(text: impl Into<String>) -> ToolCallResult {
    ToolCallResult {
        content: vec![json!({ "type": "text", "text": text.into() })],
        is_error: Some(false),
    }
}

fn error_result(message: impl Into<String>) -> ToolCallResult {
    ToolCallResult {
        content: vec![json!({ "type": "text", "text": message.into() })],
        is_error: Some(true),
    }
}
