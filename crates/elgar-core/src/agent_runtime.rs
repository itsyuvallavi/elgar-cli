use serde::{Deserialize, Serialize};

use crate::{
    agent_loop::run_permissive_agent_turn,
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
        _policy_mode: PermissionPolicyMode,
    ) -> TurnResult {
        run_permissive_agent_turn(&self.provider, session, input)
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

    use crate::{event::Event, provider::ProviderStub};

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
}
