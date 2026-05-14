use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A permissioned action owned by the controller.
///
/// An action describes what may happen later. Constructing or transitioning an
/// action is data-only and must not mutate files, call providers, or inspect
/// the filesystem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub request: ActionRequest,
    pub state: ActionLifecycleState,
    pub summary: String,
}

impl Action {
    pub fn proposed_write_file(
        id: impl Into<String>,
        target_path: impl Into<PathBuf>,
        contents: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            request: ActionRequest::WriteFile(WriteFileAction {
                target_path: target_path.into(),
                contents: contents.into(),
            }),
            state: ActionLifecycleState::Proposed,
            summary: summary.into(),
        }
    }

    pub fn kind(&self) -> ActionKind {
        match self.request {
            ActionRequest::WriteFile(_) => ActionKind::WriteFile,
        }
    }

    pub fn approve(&self) -> Self {
        if self.state == ActionLifecycleState::Proposed {
            self.with_state(ActionLifecycleState::Approved)
        } else {
            self.clone()
        }
    }

    pub fn reject(&self) -> Self {
        if self.state == ActionLifecycleState::Proposed {
            self.with_state(ActionLifecycleState::Rejected)
        } else {
            self.clone()
        }
    }

    pub fn mark_applied(&self) -> Self {
        if self.state == ActionLifecycleState::Approved {
            self.with_state(ActionLifecycleState::Applied)
        } else {
            self.clone()
        }
    }

    pub fn mark_failed(&self) -> Self {
        if self.state == ActionLifecycleState::Rejected {
            self.clone()
        } else {
            self.with_state(ActionLifecycleState::Failed)
        }
    }

    fn with_state(&self, state: ActionLifecycleState) -> Self {
        let mut action = self.clone();
        action.state = state;
        action
    }
}

/// Typed action payloads supported by the first permissioned-action slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRequest {
    WriteFile(WriteFileAction),
}

/// A proposed file write. This is a proposal only; applying it belongs to the
/// filesystem action implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFileAction {
    pub target_path: PathBuf,
    pub contents: String,
}

/// Initial action kinds that may appear in records and events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    WriteFile,
}

/// Controller-owned lifecycle states for permissioned actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionLifecycleState {
    Proposed,
    Approved,
    Applied,
    Rejected,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::{Action, ActionLifecycleState, ActionRequest};

    #[test]
    fn proposed_write_file_action_is_typed_data() {
        let action = Action::proposed_write_file(
            "action-1",
            "hello.py",
            "print('hello')\n",
            "write hello.py",
        );

        assert_eq!(action.id, "action-1");
        assert_eq!(action.state, ActionLifecycleState::Proposed);
        assert_eq!(action.summary, "write hello.py");

        let write_file = match action.request {
            ActionRequest::WriteFile(write_file) => write_file,
        };
        assert_eq!(write_file.target_path, std::path::PathBuf::from("hello.py"));
        assert_eq!(write_file.contents, "print('hello')\n");
    }

    #[test]
    fn proposed_write_file_does_not_create_target_file() {
        let target = std::env::temp_dir().join(format!(
            "elgar-proposed-action-{}-hello.py",
            std::process::id()
        ));
        let action = Action::proposed_write_file(
            "action-1",
            target.clone(),
            "print('hello')\n",
            "write hello.py",
        );

        assert_eq!(action.state, ActionLifecycleState::Proposed);
        assert!(!target.exists());
    }

    #[test]
    fn approval_is_an_in_memory_state_change_only() {
        let action = Action::proposed_write_file("action-1", "hello.py", "contents", "write file");

        let approved = action.approve();

        assert_eq!(action.state, ActionLifecycleState::Proposed);
        assert_eq!(approved.state, ActionLifecycleState::Approved);
        assert_eq!(approved.id, action.id);
    }

    #[test]
    fn rejected_actions_are_terminal() {
        let action = Action::proposed_write_file("action-1", "hello.py", "contents", "write file");
        let rejected = action.reject();

        assert_eq!(rejected.state, ActionLifecycleState::Rejected);
        assert_eq!(rejected.approve().state, ActionLifecycleState::Rejected);
        assert_eq!(
            rejected.mark_applied().state,
            ActionLifecycleState::Rejected
        );
        assert_eq!(rejected.mark_failed().state, ActionLifecycleState::Rejected);
    }

    #[test]
    fn rejected_write_file_does_not_create_target_file() {
        let target = std::env::temp_dir().join(format!(
            "elgar-rejected-action-{}-hello.py",
            std::process::id()
        ));
        let rejected =
            Action::proposed_write_file("action-1", target.clone(), "contents", "write file")
                .reject();

        assert_eq!(rejected.state, ActionLifecycleState::Rejected);
        assert!(!target.exists());
        assert_eq!(rejected.approve().state, ActionLifecycleState::Rejected);
        assert!(!target.exists());
    }

    #[test]
    fn approved_actions_can_be_marked_applied() {
        let action = Action::proposed_write_file("action-1", "hello.py", "contents", "write file");

        assert_eq!(action.mark_applied().state, ActionLifecycleState::Proposed);
        assert_eq!(
            action.approve().mark_applied().state,
            ActionLifecycleState::Applied
        );
    }

    #[test]
    fn failed_state_is_representable_for_non_rejected_actions() {
        let action = Action::proposed_write_file("action-1", "hello.py", "contents", "write file");

        assert_eq!(action.mark_failed().state, ActionLifecycleState::Failed);
    }
}
