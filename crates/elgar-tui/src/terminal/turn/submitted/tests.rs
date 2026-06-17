//! Tests for idle submitted-input handling.

use std::{
    fs,
    sync::{Arc, Mutex},
};

use elgar_core::{
    event::ProviderOutput,
    harness::PermissionMode,
    provider::{
        ChatMessage, ChatToolCall, ChatToolCallFunction, ChatToolDefinition, ControllerProvider,
        ProviderError, ProviderRequestMetadata,
    },
    session::Session,
};

use super::handle_inline_submission;
use crate::TuiShell;

#[test]
fn approve_command_executes_pending_write_request() {
    let root = test_root("approve-write");
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("tui-approve-write", &root, &root);
    let provider = ToolCallProvider::write_file("demo.txt", "hello");
    let mut shell = TuiShell::new();

    shell.submit_harness_input(&provider, &mut session, "create demo.txt");
    assert_eq!(
        session
            .pending_approval()
            .map(|approval| approval.tool.as_str()),
        Some("write")
    );

    let result = handle_inline_submission("/approve", &provider, &mut session, &mut shell)
        .expect("approve command should run");

    assert_eq!(result, (false, String::new()));
    assert_eq!(fs::read_to_string(root.join("demo.txt")).unwrap(), "hello");
    assert!(session.pending_approval().is_none());
    assert!(shell
        .conversation_copy_text()
        .contains("Done · demo.txt created"));
    assert!(!shell
        .conversation_copy_text()
        .contains("VERIFIED_WRITE_EXECUTION"));
    assert!(shell
        .raw_details_copy_text()
        .expect("raw details")
        .contains("VERIFIED_WRITE_EXECUTION"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn deny_command_clears_pending_write_request_without_execution() {
    let root = test_root("deny-write");
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("tui-deny-write", &root, &root);
    let provider = ToolCallProvider::write_file("demo.txt", "hello");
    let mut shell = TuiShell::new();

    shell.submit_harness_input(&provider, &mut session, "create demo.txt");
    assert!(session.pending_approval().is_some());

    let result = handle_inline_submission("/deny", &provider, &mut session, &mut shell)
        .expect("deny command should run");

    assert_eq!(result, (false, String::new()));
    assert!(!root.join("demo.txt").exists());
    assert!(session.pending_approval().is_none());
    assert!(shell.conversation_copy_text().contains("Denied approval-1"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn clear_command_resets_core_session_and_visible_conversation() {
    let root = test_root("clear-session");
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("terminal-tui-session", &root, &root);
    let provider = ToolCallProvider::text_response("hello back");
    let mut shell = TuiShell::new();

    shell.submit_harness_input(&provider, &mut session, "hello");
    assert!(!session.events().is_empty());

    let result = handle_inline_submission("/clear", &provider, &mut session, &mut shell)
        .expect("clear command should run");

    assert_eq!(result, (false, String::new()));
    assert!(session.events().is_empty());
    assert_eq!(session.id, "terminal-tui-session-clear-1");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn approve_command_without_pending_request_stays_local() {
    let root = test_root("approve-empty");
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("tui-approve-empty", &root, &root);
    let provider = ToolCallProvider::empty();
    let mut shell = TuiShell::new();

    let result = handle_inline_submission("/approve", &provider, &mut session, &mut shell)
        .expect("approve command should stay local");

    assert_eq!(result, (false, String::new()));
    assert!(shell
        .conversation_copy_text()
        .contains("No pending approval."));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn permissions_command_sets_workspace_write_mode() {
    let root = test_root("permissions-workspace-write");
    fs::create_dir_all(&root).unwrap();
    let mut session = Session::new("tui-permissions-workspace-write", &root, &root);
    let provider = ToolCallProvider::empty();
    let mut shell = TuiShell::new();

    let result = handle_inline_submission(
        "/permissions workspace_write",
        &provider,
        &mut session,
        &mut shell,
    )
    .expect("permissions command should run");

    assert_eq!(result, (false, String::new()));
    assert_eq!(session.permission_mode(), PermissionMode::WorkspaceWrite);
    assert!(shell
        .conversation_copy_text()
        .contains("Permission mode set to workspace_write"));
    let _ = fs::remove_dir_all(root);
}

#[derive(Clone)]
struct ToolCallProvider {
    outputs: Arc<Mutex<Vec<ProviderOutput>>>,
}

impl ToolCallProvider {
    fn empty() -> Self {
        Self {
            outputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn text_response(message: &str) -> Self {
        Self {
            outputs: Arc::new(Mutex::new(vec![ProviderOutput::new(message)])),
        }
    }

    fn write_file(path: &str, content: &str) -> Self {
        Self {
            outputs: Arc::new(Mutex::new(vec![
                ProviderOutput::new("requesting write").with_tool_calls(vec![ChatToolCall {
                    id: "call-write".to_string(),
                    tool_type: "function".to_string(),
                    function: ChatToolCallFunction {
                        name: "write".to_string(),
                        arguments: serde_json::json!({
                            "path": path,
                            "content": content
                        })
                        .to_string(),
                    },
                }]),
                ProviderOutput::new("write is pending approval"),
            ])),
        }
    }
}

impl ControllerProvider for ToolCallProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("stub", None, "stub-request")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("unused"))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        _messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        Ok(self.outputs.lock().expect("outputs lock").remove(0))
    }
}

fn test_root(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("elgar-tui-submitted-{name}-{}", std::process::id()))
}
