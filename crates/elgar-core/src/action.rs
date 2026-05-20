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
    pub fn proposed_create_file(
        id: impl Into<String>,
        target_path: impl Into<PathBuf>,
        contents: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self::proposed(
            id,
            ActionRequest::CreateFile(CreateFileAction {
                target_path: target_path.into(),
                contents: contents.into(),
            }),
            summary,
        )
    }

    pub fn proposed_write_file(
        id: impl Into<String>,
        target_path: impl Into<PathBuf>,
        contents: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self::proposed_create_file(id, target_path, contents, summary)
    }

    pub fn proposed(
        id: impl Into<String>,
        request: ActionRequest,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            request,
            state: ActionLifecycleState::Proposed,
            summary: summary.into(),
        }
    }

    pub fn kind(&self) -> ActionKind {
        match &self.request {
            ActionRequest::CreateFile(_) => ActionKind::CreateFile,
            ActionRequest::PatchFile(_) => ActionKind::PatchFile,
            ActionRequest::OverwriteFile(_) => ActionKind::OverwriteFile,
            ActionRequest::DeleteFile(_) => ActionKind::DeleteFile,
            ActionRequest::MoveFile(_) => ActionKind::MoveFile,
            ActionRequest::CreateDirectory(_) => ActionKind::CreateDirectory,
            ActionRequest::ShellCommand(_) => ActionKind::ShellCommand,
        }
    }

    pub fn approval_summary(&self) -> ApprovalSummary {
        ApprovalSummary {
            action_id: self.id.clone(),
            kind: self.kind(),
            target: self.request.approval_target(),
            summary: self.summary.clone(),
            risk_level: self.request.risk_level(),
            preview: self.request.approval_preview(),
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

/// Typed action payloads supported by the permissioned-action model.
///
/// These variants are data only. Adding a variant here does not make it
/// executable; apply behavior belongs to the filesystem or shell owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRequest {
    #[serde(alias = "WriteFile")]
    CreateFile(CreateFileAction),
    PatchFile(PatchFileAction),
    OverwriteFile(OverwriteFileAction),
    DeleteFile(DeleteFileAction),
    MoveFile(MoveFileAction),
    CreateDirectory(CreateDirectoryAction),
    ShellCommand(ShellCommandAction),
}

impl ActionRequest {
    pub fn approval_target(&self) -> String {
        match self {
            ActionRequest::CreateFile(action) => action.target_path.display().to_string(),
            ActionRequest::PatchFile(action) => action.target_path.display().to_string(),
            ActionRequest::OverwriteFile(action) => action.target_path.display().to_string(),
            ActionRequest::DeleteFile(action) => action.target_path.display().to_string(),
            ActionRequest::MoveFile(action) => format!(
                "{} -> {}",
                action.source_path.display(),
                action.target_path.display()
            ),
            ActionRequest::CreateDirectory(action) => action.target_path.display().to_string(),
            ActionRequest::ShellCommand(action) => action.command.clone(),
        }
    }

    pub fn approval_preview(&self) -> ApprovalPreview {
        match self {
            ActionRequest::CreateFile(action) => ApprovalPreview::FileContents {
                path: action.target_path.display().to_string(),
                contents: action.contents.clone(),
            },
            ActionRequest::PatchFile(action) => ApprovalPreview::Patch {
                path: action.target_path.display().to_string(),
                patch: action.patch.clone(),
            },
            ActionRequest::OverwriteFile(action) => ApprovalPreview::FileContents {
                path: action.target_path.display().to_string(),
                contents: action.contents.clone(),
            },
            ActionRequest::DeleteFile(action) => ApprovalPreview::Path {
                path: action.target_path.display().to_string(),
            },
            ActionRequest::MoveFile(action) => ApprovalPreview::Move {
                source_path: action.source_path.display().to_string(),
                target_path: action.target_path.display().to_string(),
            },
            ActionRequest::CreateDirectory(action) => ApprovalPreview::Path {
                path: action.target_path.display().to_string(),
            },
            ActionRequest::ShellCommand(action) => ApprovalPreview::ShellCommand {
                command: action.command.clone(),
                cwd: action.cwd.display().to_string(),
                timeout_seconds: action.timeout_seconds,
            },
        }
    }

    pub fn risk_level(&self) -> ActionRiskLevel {
        match self {
            ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_) => {
                ActionRiskLevel::Low
            }
            ActionRequest::PatchFile(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::MoveFile(_) => ActionRiskLevel::Medium,
            ActionRequest::DeleteFile(_) | ActionRequest::ShellCommand(_) => ActionRiskLevel::High,
        }
    }
}

/// A proposed file creation. This is a proposal only; applying it belongs to
/// the filesystem action implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateFileAction {
    pub target_path: PathBuf,
    pub contents: String,
}

pub type WriteFileAction = CreateFileAction;

/// A proposed patch to an existing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchFileAction {
    pub target_path: PathBuf,
    pub patch: String,
}

/// A proposed full replacement of an existing file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverwriteFileAction {
    pub target_path: PathBuf,
    pub contents: String,
}

/// A proposed file deletion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFileAction {
    pub target_path: PathBuf,
}

/// A proposed file move or rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveFileAction {
    pub source_path: PathBuf,
    pub target_path: PathBuf,
}

/// A proposed directory creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateDirectoryAction {
    pub target_path: PathBuf,
}

/// A proposed shell command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellCommandAction {
    pub command: String,
    pub cwd: PathBuf,
    pub timeout_seconds: u64,
}

/// Display data needed before user approval.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalSummary {
    pub action_id: String,
    pub kind: ActionKind,
    pub target: String,
    pub summary: String,
    pub risk_level: ActionRiskLevel,
    pub preview: ApprovalPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionRiskLevel {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalPreview {
    FileContents {
        path: String,
        contents: String,
    },
    Patch {
        path: String,
        patch: String,
    },
    Path {
        path: String,
    },
    Move {
        source_path: String,
        target_path: String,
    },
    ShellCommand {
        command: String,
        cwd: String,
        timeout_seconds: u64,
    },
}

/// Data shapes for verified file action results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileActionVerification {
    FileCreated {
        path: String,
    },
    FilePatched {
        path: String,
    },
    FileOverwritten {
        path: String,
    },
    FileDeleted {
        path: String,
    },
    FileMoved {
        source_path: String,
        target_path: String,
    },
    DirectoryCreated {
        path: String,
    },
}

/// Data shape for verified shell action results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellActionVerification {
    pub command: String,
    pub cwd: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub elapsed_millis: u64,
    pub timed_out: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionVerification {
    File(FileActionVerification),
    Shell(ShellActionVerification),
}

/// Action kinds that may appear in records and events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    #[serde(alias = "WriteFile")]
    CreateFile,
    PatchFile,
    OverwriteFile,
    DeleteFile,
    MoveFile,
    CreateDirectory,
    ShellCommand,
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
    use std::path::PathBuf;

    use super::{
        Action, ActionKind, ActionLifecycleState, ActionRequest, ActionRiskLevel, ApprovalPreview,
        CreateDirectoryAction, DeleteFileAction, MoveFileAction, OverwriteFileAction,
        PatchFileAction, ShellCommandAction,
    };

    #[test]
    fn proposed_create_file_action_is_typed_data() {
        let action = Action::proposed_create_file(
            "action-1",
            "hello.py",
            "print('hello')\n",
            "create hello.py",
        );

        assert_eq!(action.id, "action-1");
        assert_eq!(action.state, ActionLifecycleState::Proposed);
        assert_eq!(action.summary, "create hello.py");
        assert_eq!(action.kind(), ActionKind::CreateFile);

        let create_file = match action.request {
            ActionRequest::CreateFile(create_file) => create_file,
            other => panic!("expected CreateFile, got {other:?}"),
        };
        assert_eq!(create_file.target_path, PathBuf::from("hello.py"));
        assert_eq!(create_file.contents, "print('hello')\n");
    }

    #[test]
    fn proposed_write_file_constructor_is_compatible_create_file_data() {
        let action = Action::proposed_write_file("action-1", "hello.py", "", "create hello.py");

        assert_eq!(action.kind(), ActionKind::CreateFile);
        assert!(matches!(action.request, ActionRequest::CreateFile(_)));
    }

    #[test]
    fn old_write_file_action_json_deserializes_as_create_file() {
        let action = serde_json::from_str::<Action>(
            r#"{
                "id": "action-1",
                "request": {
                    "WriteFile": {
                        "target_path": "hello.py",
                        "contents": "print('hello')\n"
                    }
                },
                "state": "Proposed",
                "summary": "write hello.py"
            }"#,
        )
        .unwrap();

        assert_eq!(action.kind(), ActionKind::CreateFile);
        assert!(matches!(action.request, ActionRequest::CreateFile(_)));
    }

    #[test]
    fn old_write_file_action_kind_json_deserializes_as_create_file() {
        let kind = serde_json::from_str::<ActionKind>(r#""WriteFile""#).unwrap();

        assert_eq!(kind, ActionKind::CreateFile);
    }

    #[test]
    fn proposed_create_file_does_not_create_target_file() {
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
    fn expanded_action_requests_are_typed_data_only() {
        let requests = [
            ActionRequest::PatchFile(PatchFileAction {
                target_path: PathBuf::from("src/lib.rs"),
                patch: "@@ patch".to_string(),
            }),
            ActionRequest::OverwriteFile(OverwriteFileAction {
                target_path: PathBuf::from("README.md"),
                contents: "replacement".to_string(),
            }),
            ActionRequest::DeleteFile(DeleteFileAction {
                target_path: PathBuf::from("old.txt"),
            }),
            ActionRequest::MoveFile(MoveFileAction {
                source_path: PathBuf::from("old.rs"),
                target_path: PathBuf::from("new.rs"),
            }),
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: PathBuf::from("src/new"),
            }),
            ActionRequest::ShellCommand(ShellCommandAction {
                command: "cargo test".to_string(),
                cwd: PathBuf::from("."),
                timeout_seconds: 60,
            }),
        ];

        let actions = requests
            .into_iter()
            .enumerate()
            .map(|(index, request)| {
                Action::proposed(format!("action-{index}"), request, "typed data")
            })
            .collect::<Vec<_>>();

        assert_eq!(actions[0].kind(), ActionKind::PatchFile);
        assert_eq!(actions[1].kind(), ActionKind::OverwriteFile);
        assert_eq!(actions[2].kind(), ActionKind::DeleteFile);
        assert_eq!(actions[3].kind(), ActionKind::MoveFile);
        assert_eq!(actions[4].kind(), ActionKind::CreateDirectory);
        assert_eq!(actions[5].kind(), ActionKind::ShellCommand);
        assert!(actions
            .iter()
            .all(|action| action.state == ActionLifecycleState::Proposed));
    }

    #[test]
    fn approval_summary_contains_pre_approval_display_data() {
        let action = Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(ShellCommandAction {
                command: "cargo test -p elgar-core".to_string(),
                cwd: PathBuf::from("."),
                timeout_seconds: 120,
            }),
            "run core tests",
        );

        let summary = action.approval_summary();

        assert_eq!(summary.action_id, "action-1");
        assert_eq!(summary.kind, ActionKind::ShellCommand);
        assert_eq!(summary.target, "cargo test -p elgar-core");
        assert_eq!(summary.summary, "run core tests");
        assert_eq!(summary.risk_level, ActionRiskLevel::High);
        assert_eq!(
            summary.preview,
            ApprovalPreview::ShellCommand {
                command: "cargo test -p elgar-core".to_string(),
                cwd: ".".to_string(),
                timeout_seconds: 120
            }
        );
    }

    #[test]
    fn approval_does_not_change_expanded_action_request_data() {
        let action = Action::proposed(
            "action-1",
            ActionRequest::DeleteFile(DeleteFileAction {
                target_path: PathBuf::from("old.txt"),
            }),
            "delete old.txt",
        );

        let approved = action.approve();

        assert_eq!(approved.state, ActionLifecycleState::Approved);
        assert_eq!(approved.request, action.request);
        assert_eq!(approved.approval_summary().target, "old.txt");
    }

    #[test]
    fn shell_command_action_approval_does_not_execute_command() {
        let target =
            std::env::temp_dir().join(format!("elgar-shell-action-{}-marker", std::process::id()));
        let action = Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(ShellCommandAction {
                command: format!("touch {}", target.display()),
                cwd: PathBuf::from("."),
                timeout_seconds: 30,
            }),
            "touch marker",
        );

        let approved = action.approve();

        assert_eq!(approved.state, ActionLifecycleState::Approved);
        assert!(!target.exists());
    }

    #[test]
    fn verification_result_shapes_are_data_only() {
        let file_result =
            super::ActionVerification::File(super::FileActionVerification::FileMoved {
                source_path: "old.rs".to_string(),
                target_path: "new.rs".to_string(),
            });
        let shell_result = super::ActionVerification::Shell(super::ShellActionVerification {
            command: "cargo test".to_string(),
            cwd: ".".to_string(),
            stdout: "ok".to_string(),
            stderr: String::new(),
            exit_code: Some(0),
            elapsed_millis: 12,
            timed_out: false,
        });

        assert_eq!(
            file_result,
            super::ActionVerification::File(super::FileActionVerification::FileMoved {
                source_path: "old.rs".to_string(),
                target_path: "new.rs".to_string()
            })
        );
        assert_eq!(
            shell_result,
            super::ActionVerification::Shell(super::ShellActionVerification {
                command: "cargo test".to_string(),
                cwd: ".".to_string(),
                stdout: "ok".to_string(),
                stderr: String::new(),
                exit_code: Some(0),
                elapsed_millis: 12,
                timed_out: false
            })
        );
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
