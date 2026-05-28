use serde::{Deserialize, Serialize};

use crate::{
    agent_loop::{run_agent_tool_turn_with_policy, run_agent_turn_with_policy},
    context::ContextAccounting,
    controller::TurnResult,
    policy::PermissionPolicyMode,
    provider::{ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStub},
    session::Session,
};

/// Normal chat/runtime entry point for model-owned tool use.
///
/// This is the path live UI surfaces should depend on. `Controller` remains
/// available for legacy explicit-review flows, but ordinary text should enter
/// Elgar through `AgentRuntime`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRuntime<P = ProviderStub> {
    pub provider: P,
}

impl<P> AgentRuntime<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub fn refresh_context_accounting(
        &self,
        session: &mut Session,
        max_window_tokens: Option<u64>,
    ) {
        let context_accounting = ContextAccounting::from_default_local_files(
            &session.project_root,
            &session.cwd,
            max_window_tokens,
        );
        session.set_context_accounting(context_accounting);
    }
}

impl AgentRuntime<LmStudioProvider> {
    pub fn with_lm_studio_provider(config: ProviderConfig) -> Self {
        Self::new(LmStudioProvider::new(config))
    }
}

impl<P> AgentRuntime<P>
where
    P: ControllerProvider,
{
    pub fn turn(
        &self,
        session: &mut Session,
        input: &str,
        policy_mode: PermissionPolicyMode,
    ) -> TurnResult {
        run_agent_turn_with_policy(&self.provider, session, input, policy_mode)
    }

    pub fn tool_turn(
        &self,
        session: &mut Session,
        input: &str,
        policy_mode: PermissionPolicyMode,
    ) -> TurnResult {
        run_agent_tool_turn_with_policy(&self.provider, session, input, policy_mode)
    }
}

impl Default for AgentRuntime<ProviderStub> {
    fn default() -> Self {
        Self::new(ProviderStub::default())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        action::{ActionLifecycleState, ActionRequest},
        action_gate::ActionGate,
        event::{Event, ProviderOutput},
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
        policy::{ApprovalSource, PermissionPolicyMode, PolicyDecisionKind},
        provider::{
            ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata, ProviderStub,
        },
        test_env::EnvGuard,
    };

    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-agent-runtime-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn agent_runtime_runs_normal_chat_without_controller_routing() {
        let root = temp_root("normal-chat");
        let runtime = AgentRuntime::new(ProviderStub::default());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.turn(&mut session, "hello", PermissionPolicyMode::FullAccess);

        assert!(matches!(result.events.first(), Some(Event::UserMessage(_))));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(_))));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::AssistantMessage(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_applies_verified_create_actions() {
        let root = temp_root("create-directory");
        let runtime = create_directory_runtime("agent-runtime-folder");
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "create a folder called agent-runtime-folder",
            PermissionPolicyMode::FullAccess,
        );

        assert!(root.join("agent-runtime-folder").is_dir());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_review_all_proposes_create_without_mutating() {
        let root = temp_root("review-all-create");
        let runtime = create_directory_runtime("review-all-folder");
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "create a folder called review-all-folder",
            PermissionPolicyMode::ReviewAll,
        );

        assert!(!root.join("review-all-folder").exists());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        let record = session.actions().last().expect("pending action");
        assert_eq!(record.action.state, ActionLifecycleState::Proposed);
        assert_eq!(
            record
                .policy_decision
                .as_ref()
                .map(|decision| decision.kind),
            Some(PolicyDecisionKind::RequireReview)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_auto_create_records_policy_approval_not_user_approval() {
        let root = temp_root("auto-create-policy-source");
        let runtime = create_directory_runtime("auto-folder");
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "create a folder called auto-folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(root.join("auto-folder").is_dir());
        let approved = result
            .events
            .iter()
            .find_map(|event| match event {
                Event::ActionApproved(event) => Some(event),
                _ => None,
            })
            .expect("approved event");
        assert!(matches!(
            approved.approval_source,
            Some(ApprovalSource::Policy {
                mode: PermissionPolicyMode::AutoCreateReviewModify,
                ..
            })
        ));
        let record = session.actions().last().expect("applied action");
        assert!(record
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.is_policy_approved()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_auto_create_gates_shell_commands() {
        let root = temp_root("auto-create-shell");
        let runtime = shell_command_runtime("echo hello");
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "run shell command echo hello",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        let record = session.actions().last().expect("pending shell action");
        assert_eq!(record.action.state, ActionLifecycleState::Proposed);
        assert_eq!(
            record
                .policy_decision
                .as_ref()
                .map(|decision| decision.kind),
            Some(PolicyDecisionKind::RequireReview)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_auto_create_treats_missing_overwrite_as_safe_create() {
        let root = temp_root("auto-create-missing-overwrite");
        let target_file = root.join("plan.md");
        let runtime = AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::OverwriteFile),
            arguments: serde_json::json!({
                "target_path": "plan.md",
                "contents": "# Project Plan\n"
            }),
            assistant_summary: Some("write plan".to_string()),
        }));
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "create a project plan",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(
            fs::read_to_string(&target_file).unwrap(),
            "# Project Plan\n"
        );
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        assert!(result
            .events
            .iter()
            .all(|event| !matches!(event, Event::ActionProposed(_))));
        let record = session.actions().last().expect("applied action");
        assert_eq!(record.action.state, ActionLifecycleState::Applied);
        assert!(matches!(
            record.action.request,
            ActionRequest::CreateFile(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_workspace_write_policy_applies_safe_overwrite() {
        let root = temp_root("workspace-overwrite");
        fs::write(root.join("demo.txt"), "old\n").unwrap();
        let runtime = AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::OverwriteFile),
            arguments: serde_json::json!({
                "target_path": "demo.txt",
                "contents": "new\n"
            }),
            assistant_summary: Some("overwrite demo.txt".to_string()),
        }));
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            "overwrite demo.txt",
            PermissionPolicyMode::WorkspaceWriteWithReview,
        );

        assert_eq!(fs::read_to_string(root.join("demo.txt")).unwrap(), "new\n");
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        let record = session.actions().last().expect("applied overwrite");
        assert_eq!(record.action.state, ActionLifecycleState::Applied);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_workspace_write_policy_reviews_absolute_write_outside_cwd() {
        let root = temp_root("workspace-absolute-outside");
        let _home = EnvGuard::set_home(&root);
        let cwd = root.join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let target = root.join("outside.txt");
        fs::write(&target, "old\n").unwrap();
        let runtime = AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::OverwriteFile),
            arguments: serde_json::json!({
                "target_path": target,
                "contents": "new\n"
            }),
            assistant_summary: Some("overwrite outside workspace".to_string()),
        }));
        let mut session = Session::new("session-1", &root, &cwd);

        let result = runtime.tool_turn(
            &mut session,
            "overwrite outside.txt",
            PermissionPolicyMode::WorkspaceWriteWithReview,
        );

        assert_eq!(fs::read_to_string(&target).unwrap(), "old\n");
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert!(!result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        let record = session.actions().last().expect("pending overwrite");
        assert_eq!(record.action.state, ActionLifecycleState::Proposed);
        assert_eq!(
            record
                .policy_decision
                .as_ref()
                .map(|decision| decision.kind),
            Some(PolicyDecisionKind::RequireReview)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_workspace_write_policy_applies_absolute_write_inside_cwd() {
        let root = temp_root("workspace-absolute-inside");
        let cwd = root.join("workspace");
        fs::create_dir_all(&cwd).unwrap();
        let target = cwd.join("demo.txt");
        fs::write(&target, "old\n").unwrap();
        let runtime = AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::OverwriteFile),
            arguments: serde_json::json!({
                "target_path": target,
                "contents": "new\n"
            }),
            assistant_summary: Some("overwrite workspace file".to_string()),
        }));
        let mut session = Session::new("session-1", &root, &cwd);

        let result = runtime.tool_turn(
            &mut session,
            "overwrite demo.txt",
            PermissionPolicyMode::WorkspaceWriteWithReview,
        );

        assert_eq!(fs::read_to_string(cwd.join("demo.txt")).unwrap(), "new\n");
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_review_all_prefers_file_action_from_directory_file_batch() {
        let root = temp_root("review-all-file-batch");
        let _home = EnvGuard::set_home(&root);
        let target_dir = root.join("ElgarPermissionTest");
        let target_file = target_dir.join("test.txt");
        let provider = BatchToolProvider::new(vec![
            RawModelToolCall {
                id: "tool-dir".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateDirectory),
                arguments: serde_json::json!({
                    "target_path": target_dir
                }),
                assistant_summary: Some("create parent directory".to_string()),
            },
            RawModelToolCall {
                id: "tool-file".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": target_file,
                    "contents": "hello"
                }),
                assistant_summary: Some("create test.txt".to_string()),
            },
        ]);
        let runtime = AgentRuntime::new(provider.clone());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            &format!(
                "create a file called test.txt inside {} with hello",
                target_dir.display()
            ),
            PermissionPolicyMode::ReviewAll,
        );

        assert!(!target_file.exists());
        assert_eq!(provider.call_count(), 1);
        assert_eq!(session.actions().len(), 1);
        let record = session.actions().last().expect("pending action");
        assert_eq!(record.action.state, ActionLifecycleState::Proposed);
        let ActionRequest::CreateFile(create_file) = &record.action.request else {
            panic!("expected pending create file action");
        };
        assert_eq!(create_file.target_path, target_file);
        assert!(result
            .events
            .iter()
            .all(|event| !matches!(event, Event::AssistantMessage(message) if message.content.contains("approve"))));

        ActionGate::new(ProviderStub::default()).approve(&mut session);

        assert_eq!(
            session.actions().last().unwrap().failure_reason,
            None,
            "approval should not fail"
        );
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "hello");
        assert_eq!(
            session.actions().last().unwrap().action.state,
            ActionLifecycleState::Applied
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_review_all_reports_existing_same_file_without_pending_action() {
        let root = temp_root("review-all-existing-same-file");
        let target_dir = root.join("ElgarPermissionTest");
        let target_file = target_dir.join("test.txt");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(&target_file, "hello").unwrap();
        let provider = BatchToolProvider::new(vec![RawModelToolCall {
            id: "tool-file".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: serde_json::json!({
                "target_path": target_file,
                "contents": "hello"
            }),
            assistant_summary: Some("create test.txt".to_string()),
        }]);
        let runtime = AgentRuntime::new(provider.clone());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            &format!(
                "create a file called test.txt inside {} with hello",
                target_dir.display()
            ),
            PermissionPolicyMode::ReviewAll,
        );

        assert_eq!(provider.call_count(), 1);
        assert!(session.actions().is_empty());
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "hello");
        assert!(result.events.iter().any(|event| {
            matches!(event, Event::AssistantMessage(message) if message.content.contains("already exists with the requested content"))
        }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_runtime_review_all_turns_existing_different_create_into_overwrite_review() {
        let root = temp_root("review-all-existing-different-file");
        let target_dir = root.join("ElgarPermissionTest");
        let target_file = target_dir.join("test.txt");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(&target_file, "old").unwrap();
        let provider = BatchToolProvider::new(vec![RawModelToolCall {
            id: "tool-file".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateFile),
            arguments: serde_json::json!({
                "target_path": target_file,
                "contents": "hello"
            }),
            assistant_summary: Some("create test.txt".to_string()),
        }]);
        let runtime = AgentRuntime::new(provider.clone());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.tool_turn(
            &mut session,
            &format!(
                "create a file called test.txt inside {} with hello",
                target_dir.display()
            ),
            PermissionPolicyMode::ReviewAll,
        );

        assert_eq!(provider.call_count(), 1);
        assert_eq!(session.actions().len(), 1);
        let record = session.actions().last().expect("pending action");
        assert_eq!(record.action.state, ActionLifecycleState::Proposed);
        let ActionRequest::OverwriteFile(overwrite_file) = &record.action.request else {
            panic!("expected overwrite review for existing file");
        };
        assert_eq!(overwrite_file.target_path, target_file);
        assert_eq!(overwrite_file.contents, "hello");
        assert_eq!(fs::read_to_string(&target_file).unwrap(), "old");
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(action) if action.target.as_deref() == Some(target_file.to_str().unwrap()))));

        ActionGate::new(ProviderStub::default()).approve(&mut session);

        assert_eq!(fs::read_to_string(&target_file).unwrap(), "hello");
        assert_eq!(
            session.actions().last().unwrap().action.state,
            ActionLifecycleState::Applied
        );

        let _ = fs::remove_dir_all(root);
    }

    #[derive(Debug, Clone)]
    struct ToolProvider {
        call: RawModelToolCall,
    }

    impl ToolProvider {
        fn new(call: RawModelToolCall) -> Self {
            Self { call }
        }
    }

    fn create_directory_runtime(target_path: &str) -> AgentRuntime<ToolProvider> {
        AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::CreateDirectory),
            arguments: serde_json::json!({
                "target_path": target_path
            }),
            assistant_summary: Some(format!("create {target_path}")),
        }))
    }

    fn shell_command_runtime(command: &str) -> AgentRuntime<ToolProvider> {
        AgentRuntime::new(ToolProvider::new(RawModelToolCall {
            id: "tool-1".to_string(),
            name: RawModelToolName::Known(ModelToolName::ShellCommand),
            arguments: serde_json::json!({
                "command": command,
                "cwd": "."
            }),
            assistant_summary: Some(format!("run {command}")),
        }))
    }

    impl ControllerProvider for ToolProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("tool-provider", None, "tool-request-1")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("tool provider"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if messages
                .iter()
                .any(|message| matches!(message.role, crate::provider::ChatRole::Tool))
            {
                return Ok(ProviderOutput::new("Done."));
            }

            Ok(ProviderOutput::new("Using a tool.").with_tool_calls(vec![self.call.clone()]))
        }
    }

    #[derive(Debug, Clone)]
    struct BatchToolProvider {
        calls: Vec<RawModelToolCall>,
        call_count: std::sync::Arc<std::sync::Mutex<usize>>,
    }

    impl BatchToolProvider {
        fn new(calls: Vec<RawModelToolCall>) -> Self {
            Self {
                calls,
                call_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            }
        }

        fn call_count(&self) -> usize {
            *self.call_count.lock().unwrap()
        }
    }

    impl ControllerProvider for BatchToolProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("batch-tool-provider", None, "tool-request-1")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("batch tool provider"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            let mut count = self.call_count.lock().unwrap();
            *count += 1;
            if *count > 1 {
                return Ok(ProviderOutput::new(
                    "Do you approve creating the directory and file?",
                ));
            }

            Ok(
                ProviderOutput::new("Do you approve creating the directory and file?")
                    .with_tool_calls(self.calls.clone()),
            )
        }
    }
}
