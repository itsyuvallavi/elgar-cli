use serde::{Deserialize, Serialize};

use crate::{
    agent_loop::run_agent_turn_with_policy,
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
        action::ActionLifecycleState,
        event::{Event, ProviderOutput},
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
        policy::{ApprovalSource, PermissionPolicyMode, PolicyDecisionKind},
        provider::{
            ChatMessage, ChatToolDefinition, ControllerProvider, ProviderError,
            ProviderRequestMetadata, ProviderStub,
        },
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
        let runtime = AgentRuntime::new(ProviderStub::default());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.turn(
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
        let runtime = AgentRuntime::new(ProviderStub::default());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.turn(
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
        let runtime = AgentRuntime::new(ProviderStub::default());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.turn(
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
        let runtime = AgentRuntime::new(ProviderStub::default());
        let mut session = Session::new("session-1", &root, &root);

        let result = runtime.turn(
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

        let result = runtime.turn(
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

    #[derive(Debug, Clone)]
    struct ToolProvider {
        call: RawModelToolCall,
    }

    impl ToolProvider {
        fn new(call: RawModelToolCall) -> Self {
            Self { call }
        }
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
}
