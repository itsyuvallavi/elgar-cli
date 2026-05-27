use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use elgar_core::{
    action_gate::ActionGate,
    agent_runtime::AgentRuntime,
    event::{Event, ProviderOutput},
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    policy::PermissionPolicyMode,
    provider::{
        ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError,
        ProviderRequestMetadata, ProviderStub,
    },
    router::{route_input, Route},
    session::Session,
};
use serde_json::json;

fn regression_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "elgar-core-regression-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn session_at(root: &Path) -> Session {
    Session::new("session-1", root, root)
}

#[test]
fn ordinary_words_are_model_input_not_controller_commands() {
    assert_eq!(route_input("help"), Route::AskModel);
    assert_eq!(route_input("approve"), Route::AskModel);
    assert_eq!(route_input("ok"), Route::AskModel);
    assert_eq!(route_input("create file hello.py"), Route::AskModel);
    assert_eq!(route_input("create a folder called demo"), Route::AskModel);

    assert_eq!(route_input("/help"), Route::Help);
    assert_eq!(route_input("/approve"), Route::ApproveAction);
    assert_eq!(route_input("/reject"), Route::RejectAction);
}

#[test]
fn plain_runtime_file_request_stays_provider_chat_without_mutation() {
    let root = regression_root("plain-file-request");
    let runtime = AgentRuntime::new(ProviderStub::default());
    let mut session = session_at(&root);

    let result = runtime.turn(
        &mut session,
        "create file hello.py",
        PermissionPolicyMode::FullAccess,
    );

    assert!(!root.join("hello.py").exists());
    assert!(session.actions().is_empty());
    assert!(result.events.iter().any(
        |event| matches!(event, Event::ProviderStarted(started) if started.tool_count == Some(0))
    ));
    assert!(result
        .events
        .iter()
        .all(|event| !matches!(event, Event::ActionProposed(_) | Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_tool_turn_applies_provider_requested_create_file() {
    let root = regression_root("tool-create-file");
    let runtime = AgentRuntime::new(SingleToolCallProvider::new(raw_tool_call(
        "call-create-file",
        ModelToolName::CreateFile,
        json!({"target_path": "hello.py", "contents": "hello\n"}),
    )));
    let mut session = session_at(&root);

    let result = runtime.tool_turn(
        &mut session,
        "create file hello.py",
        PermissionPolicyMode::FullAccess,
    );

    assert_eq!(
        fs::read_to_string(root.join("hello.py")).unwrap(),
        "hello\n"
    );
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ProviderStarted(started) if started.tool_count.unwrap_or_default() > 0)));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn explicit_tool_turn_review_all_waits_for_slash_approval() {
    let root = regression_root("tool-review-all");
    let runtime = AgentRuntime::new(SingleToolCallProvider::new(raw_tool_call(
        "call-create-file",
        ModelToolName::CreateFile,
        json!({"target_path": "hello.py", "contents": "hello\n"}),
    )));
    let gate = ActionGate::default();
    let mut session = session_at(&root);

    runtime.tool_turn(
        &mut session,
        "create file hello.py",
        PermissionPolicyMode::ReviewAll,
    );

    assert!(!root.join("hello.py").exists());
    assert_eq!(session.actions().len(), 1);
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionProposed(_))));

    let approved = gate.approve(&mut session);

    assert_eq!(approved.route, Route::ApproveAction);
    assert_eq!(
        fs::read_to_string(root.join("hello.py")).unwrap(),
        "hello\n"
    );
    assert!(session
        .events()
        .iter()
        .any(|event| matches!(event, Event::ActionApplied(_))));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn action_gate_rejects_typed_pending_action_without_controller_phrase_routing() {
    let root = regression_root("typed-reject");
    let runtime = AgentRuntime::new(SingleToolCallProvider::new(raw_tool_call(
        "call-create-directory",
        ModelToolName::CreateDirectory,
        json!({"target_path": "demo"}),
    )));
    let gate = ActionGate::default();
    let mut session = session_at(&root);
    runtime.tool_turn(
        &mut session,
        "create directory demo",
        PermissionPolicyMode::ReviewAll,
    );

    let result = gate.reject(&mut session);

    assert_eq!(result.route, Route::RejectAction);
    assert!(!root.join("demo").exists());
    assert!(matches!(
        result.events.first(),
        Some(Event::UserMessage(message)) if message.content == "/reject"
    ));
    assert!(result
        .events
        .iter()
        .any(|event| matches!(event, Event::ActionRejected(_))));

    let _ = fs::remove_dir_all(root);
}

fn raw_tool_call(
    id: impl Into<String>,
    name: ModelToolName,
    arguments: serde_json::Value,
) -> RawModelToolCall {
    RawModelToolCall {
        id: id.into(),
        name: RawModelToolName::Known(name),
        arguments,
        assistant_summary: None,
    }
}

#[derive(Debug, Clone)]
struct SingleToolCallProvider {
    call: RawModelToolCall,
    tool_counts: Arc<Mutex<Vec<usize>>>,
}

impl SingleToolCallProvider {
    fn new(call: RawModelToolCall) -> Self {
        Self {
            call,
            tool_counts: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ControllerProvider for SingleToolCallProvider {
    fn request_metadata(&self) -> ProviderRequestMetadata {
        ProviderRequestMetadata::new("tool-provider", Some("tool-model".to_string()), "request-1")
    }

    fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("plain response"))
    }

    fn chat_messages_with_metadata(
        &self,
        _messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
    ) -> Result<ProviderOutput, ProviderError> {
        Ok(ProviderOutput::new("plain response"))
    }

    fn chat_messages_with_tools_with_metadata(
        &self,
        _messages: Vec<ChatMessage>,
        _metadata: &ProviderRequestMetadata,
        tools: Vec<ChatToolDefinition>,
    ) -> Result<ProviderOutput, ProviderError> {
        self.tool_counts.lock().unwrap().push(tools.len());
        Ok(ProviderOutput::new("").with_tool_calls(vec![self.call.clone()]))
    }
}
