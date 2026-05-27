use std::{
    fs,
    path::{Path, PathBuf},
};

use elgar_core::{
    action::ActionLifecycleState,
    action_gate::ActionGate,
    agent_runtime::AgentRuntime,
    controller::Controller,
    event::{Event, ProviderOutput, VerifiedActionResult},
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    policy::PermissionPolicyMode,
    provider::{
        ChatMessage, ChatRole, ChatToolDefinition, ControllerProvider, ProviderError,
        ProviderRequestMetadata, ProviderStub,
    },
    session::Session,
};
use elgar_tui::TuiShell;

fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-tui-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("tui-smoke-session", root, root)
}

#[test]
fn renders_initial_state() {
    let rendered = TuiShell::new().render();

    assert!(rendered.contains("Conversation\n(empty conversation)"));
    assert!(rendered.contains("Pending Action\nnone"));
    assert!(rendered.contains("Status\nready"));
}

#[test]
fn renders_core_chat_events_without_action_truth() {
    let controller = Controller::new(ProviderStub::default());
    let root = smoke_root("chat-events");
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what does the harness do?");
    shell.consume_events(&result.events);

    let rendered = shell.render();
    assert!(rendered.contains("> what does the harness do?"));
    assert!(rendered.contains("stub provider response"));
    assert!(session.actions().is_empty());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plain_agent_input_renders_provider_text_without_mutating_files() {
    let root = smoke_root("plain-agent-input");
    let runtime = AgentRuntime::new(ProviderStub::default());
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    shell.submit_agent_input(&runtime, &mut session, "create file hello.py");

    assert!(!root.join("hello.py").exists());
    assert!(session.actions().is_empty());
    assert!(shell.render().contains("stub provider response"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_tool_turn_can_be_approved_through_action_gate() {
    let root = smoke_root("approve-tool");
    let runtime = create_file_runtime("hello.py", "hello\n");
    let gate = ActionGate::default();
    let mut session = session_at(&root);
    let mut shell = TuiShell::with_policy_mode(PermissionPolicyMode::ReviewAll);

    shell.submit_agent_tool_input(&runtime, &mut session, "create file hello.py");

    assert!(!root.join("hello.py").exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Proposed
    );
    assert!(shell.render().contains("Status: waiting for approval"));

    shell.submit_approval(&gate, &mut session);

    assert_eq!(
        fs::read_to_string(root.join("hello.py")).unwrap(),
        "hello\n"
    );
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(matches!(
        session.actions()[0].verified_result,
        Some(VerifiedActionResult::FileWritten { .. })
    ));
    assert!(shell.render().contains("Status: applied and verified"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_tool_turn_can_be_rejected_without_writing() {
    let root = smoke_root("reject-tool");
    let runtime = create_file_runtime("rejected.py", "");
    let gate = ActionGate::default();
    let mut session = session_at(&root);
    let mut shell = TuiShell::with_policy_mode(PermissionPolicyMode::ReviewAll);

    shell.submit_agent_tool_input(&runtime, &mut session, "create file rejected.py");
    shell.submit_rejection(&gate, &mut session);

    assert!(!root.join("rejected.py").exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert!(session
        .events()
        .iter()
        .all(|event| !matches!(event, Event::ActionApplied(_))));
    assert!(shell.render().contains("Status: rejected"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_tool_call_repairs_without_raw_validation_text() {
    let root = smoke_root("invalid-tool-guidance");
    let runtime = AgentRuntime::new(ScriptedToolProvider {
        output: ProviderOutput::new("Creating file.").with_tool_calls(vec![RawModelToolCall {
            id: "call-missing-target".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: serde_json::json!({ "contents": "hello\n" }),
            assistant_summary: None,
        }]),
    });
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    shell.submit_agent_tool_input(&runtime, &mut session, "create a file");

    let rendered = shell.render();
    assert!(session.actions().is_empty());
    assert!(rendered.contains("Done."));
    assert!(!rendered.contains("I need a concrete target path before I can create the file"));
    assert!(!rendered.contains("missing required argument"));
    assert!(!rendered.contains("Tool call incomplete"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_error_renders_without_mutating_action_truth() {
    let root = smoke_root("provider-error");
    let controller = Controller::new(FailingProvider);
    let mut session = session_at(&root);
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what happened?");
    shell.consume_events(&result.events);

    let rendered = shell.render();
    assert!(rendered.contains("fake-provider request fake-request-1 failed"));
    assert!(rendered.contains("model missing"));
    assert!(session.actions().is_empty());

    let _ = fs::remove_dir_all(root);
}

fn create_file_runtime(
    target_path: impl Into<String>,
    contents: impl Into<String>,
) -> AgentRuntime<ScriptedToolProvider> {
    AgentRuntime::new(ScriptedToolProvider {
        output: ProviderOutput::new("Creating file.").with_tool_calls(vec![RawModelToolCall {
            id: "call-create-file".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: serde_json::json!({
                "target_path": target_path.into(),
                "contents": contents.into(),
            }),
            assistant_summary: Some("create file".to_string()),
        }]),
    })
}

#[derive(Debug, Clone)]
struct ScriptedToolProvider {
    output: ProviderOutput,
}

impl ControllerProvider for ScriptedToolProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("tool-provider", Some("tool-model".to_string()), "request-1")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("plain response"))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        _tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        if messages
            .iter()
            .any(|message| matches!(message.role, ChatRole::Tool))
        {
            return Ok(ProviderOutput::new("Done."));
        }

        Ok(self.output.clone())
    }
}

#[derive(Debug, Clone)]
struct FailingProvider;

impl ControllerProvider for FailingProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new(
            "fake-provider",
            Some("fake-model".to_string()),
            "fake-request-1",
        )
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Err(ProviderError::provider("model missing", Some(404), None))
    }
}
