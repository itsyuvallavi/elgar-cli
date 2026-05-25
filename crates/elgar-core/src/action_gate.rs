use serde::{Deserialize, Serialize};

use crate::{
    controller::{Controller, TurnResult},
    provider::{ControllerProvider, ProviderStub},
    session::Session,
};

/// Narrow approval/rejection gate used after the model/runtime proposes work.
///
/// Normal user text must enter Elgar through `AgentRuntime`. This type exists
/// only for explicit action lifecycle commands such as `/approve` and `/reject`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionGate<P = ProviderStub> {
    legacy_controller: Controller<P>,
}

impl<P> ActionGate<P> {
    pub fn new(provider: P) -> Self {
        Self {
            legacy_controller: Controller::new(provider),
        }
    }
}

impl<P> ActionGate<P>
where
    P: ControllerProvider,
{
    pub fn approve(&self, session: &mut Session) -> TurnResult {
        self.legacy_controller.turn(session, "approve")
    }

    pub fn reject(&self, session: &mut Session) -> TurnResult {
        self.legacy_controller.turn(session, "reject")
    }
}

impl Default for ActionGate<ProviderStub> {
    fn default() -> Self {
        Self::new(ProviderStub::default())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::{
        action::{Action, ActionRequest, CreateDirectoryAction},
        event::Event,
        session::{ActionRecord, Session},
    };

    use super::*;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-action-gate-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn action_gate_applies_explicit_approval_only() {
        let root = temp_root("approve");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: "demo".into(),
            }),
            "create demo",
        )));

        let result = gate.approve(&mut session);

        assert!(root.join("demo").is_dir());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn action_gate_rejects_explicit_pending_action() {
        let root = temp_root("reject");
        let gate = ActionGate::default();
        let mut session = Session::new("session-1", &root, &root);
        session.push_action(ActionRecord::new(Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: "demo".into(),
            }),
            "create demo",
        )));

        let result = gate.reject(&mut session);

        assert!(!root.join("demo").exists());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionRejected(_))));

        let _ = fs::remove_dir_all(root);
    }
}
