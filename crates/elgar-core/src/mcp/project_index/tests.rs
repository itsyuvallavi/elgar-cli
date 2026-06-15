//! Tests for internal Project Index MCP tools.

use std::fs;

use serde_json::json;

use super::{call_project_index_tool, project_index_tools};
use crate::session::Session;

#[test]
fn project_index_advertises_expected_tools() {
    let tools = project_index_tools();
    let names = tools
        .tools
        .iter()
        .map(|tool| tool.name.as_str())
        .collect::<Vec<_>>();

    assert!(names.contains(&"project_tree"));
    assert!(names.contains(&"project_find"));
    assert!(names.contains(&"project_read_summary"));
    assert!(names.contains(&"project_status"));
}

#[test]
fn project_index_rejects_parent_paths() {
    let root = std::env::temp_dir().join(format!(
        "elgar-project-index-parent-test-{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let session = Session::new("project-index-parent-session", &root, &root);

    let result = call_project_index_tool(
        &session,
        "project_read_summary",
        &json!({ "path": "../secret.txt" }),
    );

    assert_eq!(result.is_error, Some(true));
    assert!(result.content[0]["text"]
        .as_str()
        .is_some_and(|text| text.contains("parent-directory")));
    let _ = fs::remove_dir_all(root);
}
