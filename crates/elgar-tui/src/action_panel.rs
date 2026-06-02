use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use elgar_core::event::{ActionEvent, Event, FileActionVerification, VerifiedActionResult};
use elgar_core::policy::ApprovalSource;

use crate::shell_result::render_shell_execution_summary;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PendingActionArea {
    pub panel: Option<ActionApprovalPanel>,
    hidden_policy_actions: HashSet<String>,
}

impl PendingActionArea {
    pub fn observe_event(&mut self, event: &Event) {
        match event {
            Event::ActionProposed(action) => {
                self.panel = Some(ActionApprovalPanel::pending(action));
            }
            Event::ActionApproved(action) => {
                if action
                    .approval_source
                    .as_ref()
                    .is_some_and(ApprovalSource::is_policy)
                {
                    self.hidden_policy_actions.insert(action.action_id.clone());
                    if self
                        .panel
                        .as_ref()
                        .is_some_and(|panel| panel.action_id == action.action_id)
                    {
                        self.panel = None;
                    }
                    return;
                }
                self.update_or_replace(action, ActionPanelState::Approved, None)
            }
            Event::ActionRejected(action) => self.update_or_replace(
                action,
                ActionPanelState::Rejected,
                Some(rejected_result_text(action)),
            ),
            Event::ActionApplied(action) => {
                if self.hidden_policy_actions.remove(&action.action_id) {
                    if self
                        .panel
                        .as_ref()
                        .is_some_and(|panel| panel.action_id == action.action_id)
                    {
                        self.panel = None;
                    }
                    return;
                }

                self.update_result(
                    &action.action_id,
                    ActionPanelState::Applied,
                    Some(render_verified_result(&action.result)),
                )
            }
            Event::ActionFailed(action) => self.update_result(
                &action.action_id,
                ActionPanelState::Failed,
                Some(action.reason.clone()),
            ),
            _ => {}
        }
    }

    pub(crate) fn render_body(&self) -> String {
        self.panel
            .as_ref()
            .map(ActionApprovalPanel::render)
            .unwrap_or_else(|| "none".to_string())
    }

    fn update_or_replace(
        &mut self,
        action: &ActionEvent,
        state: ActionPanelState,
        result: Option<String>,
    ) {
        if let Some(panel) = self
            .panel
            .as_mut()
            .filter(|panel| panel.action_id == action.action_id)
        {
            panel.state = state;
            panel.result = result;
        } else {
            let mut panel = ActionApprovalPanel::pending(action);
            panel.state = state;
            panel.result = result;
            self.panel = Some(panel);
        }
    }

    fn update_result(&mut self, action_id: &str, state: ActionPanelState, result: Option<String>) {
        if let Some(panel) = self
            .panel
            .as_mut()
            .filter(|panel| panel.action_id == action_id)
        {
            panel.state = state;
            panel.result = result;
        } else {
            self.panel = Some(ActionApprovalPanel {
                action_id: action_id.to_string(),
                action_type: "unknown".to_string(),
                target: None,
                cwd: None,
                timeout_seconds: None,
                summary: "not available from this result".to_string(),
                state,
                result,
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionApprovalPanel {
    pub action_id: String,
    pub action_type: String,
    pub target: Option<String>,
    pub cwd: Option<String>,
    pub timeout_seconds: Option<u64>,
    pub summary: String,
    pub state: ActionPanelState,
    pub result: Option<String>,
}

impl ActionApprovalPanel {
    fn pending(action: &ActionEvent) -> Self {
        Self {
            action_id: action.action_id.clone(),
            action_type: format!("{:?}", action.action_kind),
            target: action.target.clone(),
            cwd: action
                .shell_details
                .as_ref()
                .map(|details| details.cwd.clone()),
            timeout_seconds: action
                .shell_details
                .as_ref()
                .map(|details| details.timeout_seconds),
            summary: action.summary.clone(),
            state: ActionPanelState::Proposed,
            result: None,
        }
    }

    fn render(&self) -> String {
        let mut lines = Vec::new();
        if self.action_type == "ShellCommand" && self.state == ActionPanelState::Proposed {
            lines.push("Model proposed a shell command; it has not run yet.".to_string());
        }
        if self.action_type == "ShellCommand" && self.state == ActionPanelState::Approved {
            lines.push("Local executor is running this shell command.".to_string());
        }
        lines.push(format!("Status: {}", self.state.render()));

        if let Some(request) = self.request_line() {
            lines.push(request);
        }
        if self.action_type == "ShellCommand" {
            if let Some(cwd) = self.cwd.as_deref() {
                lines.push(format!("Cwd: {}", user_display_path(cwd)));
            }
            if let Some(timeout_seconds) = self.timeout_seconds {
                lines.push(format!("Timeout: {timeout_seconds}s"));
            }
        }

        if let Some(result) = &self.result {
            lines.push(format!("Result: {result}"));
        }

        if self.state == ActionPanelState::Proposed {
            lines.push("[ Approve ]  [ Reject ]".to_string());
        }
        lines.push(self.state.instructions().to_string());
        lines.join("\n")
    }

    fn request_line(&self) -> Option<String> {
        if let Some(path) = self.summary.strip_prefix("write ") {
            return Some(format!("File: {}", path.trim()));
        }

        if let Some(path) = self.summary.strip_prefix("create directory ") {
            return Some(format!("Folder: {}", user_display_path(path.trim())));
        }

        if let Some(path) = self.summary.strip_prefix("delete ") {
            return Some(format!("File: {}", user_display_path(path.trim())));
        }

        if let Some(paths) = self.summary.strip_prefix("move ") {
            return Some(format!("Move: {}", paths.trim()));
        }

        if let Some(path) = self.summary.strip_prefix("create Markdown plan ") {
            return Some(format!("Plan: {}", user_display_path(path.trim())));
        }

        if let Some(path) = self.summary.strip_prefix("execute Markdown plan in ") {
            return Some(format!(
                "Project folder: {}",
                user_display_path(path.trim())
            ));
        }

        if let Some(command) = self
            .target
            .as_deref()
            .or_else(|| self.summary.strip_prefix("run shell command "))
        {
            if self.action_type == "ShellCommand" {
                return Some(format!("Command: {}", command.trim()));
            }
        }

        if self.summary.trim().is_empty() {
            None
        } else {
            Some(format!("Request: {}", self.summary.trim()))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPanelState {
    Proposed,
    Approved,
    Applied,
    Rejected,
    Failed,
}

impl ActionPanelState {
    fn render(self) -> &'static str {
        match self {
            Self::Proposed => "waiting for approval",
            Self::Approved => "approved",
            Self::Applied => "applied and verified",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    fn instructions(self) -> &'static str {
        match self {
            Self::Proposed => {
                "No changes have been made yet. Use /approve to apply or /reject to leave it unchanged."
            }
            Self::Approved => "Approved. Applying it now.",
            Self::Applied => "Verified.",
            Self::Rejected => "Rejected. Nothing was changed.",
            Self::Failed => "Failed before verification completed.",
        }
    }
}

fn rejected_result_text(action: &ActionEvent) -> String {
    if action.action_kind == elgar_core::event::ActionKind::ShellCommand {
        "Rejected. The shell command was not run.".to_string()
    } else {
        "Rejected. No file was changed.".to_string()
    }
}

fn render_verified_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("Wrote {}.", user_display_path(path))
        }
        VerifiedActionResult::File(file) => render_file_verification(file),
        VerifiedActionResult::Shell(shell) => {
            if let Some(effect) = &shell.verified_effect {
                if let Some(message) = render_shell_verified_effect(effect) {
                    return message;
                }
            }
            render_shell_execution_summary(shell)
        }
    }
}

fn render_shell_verified_effect(effect: &str) -> Option<String> {
    if let Some(path) = verified_effect_value(effect, "verified file exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified files exist: ") {
        return Some(format!("Created files: {}.", user_display_path_list(paths)));
    }

    if let Some(path) = verified_effect_value(effect, "verified directory exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified directories exist: ") {
        return Some(format!("Created {}.", user_display_path_list(paths)));
    }

    None
}

fn verified_effect_value<'a>(effect: &'a str, prefix: &str) -> Option<&'a str> {
    effect
        .split("; ")
        .find_map(|part| part.strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn render_file_verification(result: &FileActionVerification) -> String {
    match result {
        FileActionVerification::FileCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
        FileActionVerification::FilePatched { path } => {
            format!("Updated {}.", user_display_path(path))
        }
        FileActionVerification::FileOverwritten { path } => {
            format!("Overwrote {}.", user_display_path(path))
        }
        FileActionVerification::FileDeleted { path } => {
            format!("Deleted {}.", user_display_path(path))
        }
        FileActionVerification::FileMoved {
            source_path,
            target_path,
        } => format!(
            "Moved {} to {}.",
            user_display_path(source_path),
            user_display_path(target_path)
        ),
        FileActionVerification::DirectoryCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
    }
}

fn user_display_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let desktop = home.join("Desktop");
        if path == desktop {
            return "Desktop".to_string();
        }
        if let Ok(relative) = path.strip_prefix(&desktop) {
            return PathBuf::from("Desktop")
                .join(relative)
                .display()
                .to_string();
        }
    }

    path.display().to_string()
}

fn user_display_path_list(paths: &str) -> String {
    paths
        .split(", ")
        .map(user_display_path)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use elgar_core::event::{
        ActionApplied, ActionEvent, Event, ShellActionVerification, VerifiedActionResult,
    };

    use super::{ActionPanelState, PendingActionArea};

    #[test]
    fn pending_action_area_shows_proposed_action_from_core_event() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let panel = pending_action.panel.as_ref().unwrap();
        assert_eq!(panel.action_id, "action-1");
        assert_eq!(panel.action_type, "CreateFile");
        assert_eq!(panel.target.as_deref(), Some("hello.py"));
        assert_eq!(panel.summary, "write hello.py");
        assert_eq!(panel.state, ActionPanelState::Proposed);

        let rendered = pending_action.render_body();
        assert!(rendered.contains("File: hello.py"));
        assert!(rendered.contains("Status: waiting for approval"));
        assert!(rendered.contains("[ Approve ]  [ Reject ]"));
        assert!(rendered.contains("No changes have been made yet."));
        assert!(rendered.contains("Use /approve to apply or /reject"));
        assert!(!rendered.contains("Action: action-1 CreateFile"));
        assert!(!rendered.contains("Summary: write hello.py"));
    }

    #[test]
    fn terminal_action_events_render_result_state() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));
        pending_action.observe_event(&Event::ActionRejected(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Status: rejected"));
        assert!(rendered.contains("Result: Rejected. No file was changed."));
        assert!(rendered.contains("Rejected. Nothing was changed."));
    }

    #[test]
    fn proposed_shell_action_shows_command_cwd_timeout_and_buttons() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::ShellCommand,
                "run shell command npm create",
            )
            .with_target("npm create vite@latest demo -- --template react-ts")
            .with_shell_details("/repo", 300, "Scaffold the project."),
        ));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Command: npm create vite@latest demo -- --template react-ts"));
        assert!(rendered.contains("Cwd: /repo"));
        assert!(rendered.contains("Timeout: 300s"));
        assert!(rendered.contains("[ Approve ]  [ Reject ]"));
    }

    #[test]
    fn applied_result_without_current_panel_uses_fallback_details() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "hello.py".to_string(),
            },
        )));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Status: applied and verified"));
        assert!(rendered.contains("Result: Wrote hello.py."));
    }

    #[test]
    fn applied_shell_result_renders_timeout_summary_and_hides_stderr_details() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::ShellCommand,
            VerifiedActionResult::Shell(ShellActionVerification {
                command: "sleep 60".to_string(),
                cwd: "/repo".to_string(),
                stdout: String::new(),
                stderr: "timed out\n".to_string(),
                stdout_truncated: false,
                stderr_truncated: true,
                exit_code: None,
                elapsed_millis: 30_000,
                timed_out: true,
                verified_effect: None,
            }),
        )));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Status: applied and verified"));
        assert!(rendered.contains("shell command timed out · 30.0s"));
        assert!(rendered.contains("stderr hidden (truncated)"));
        assert!(rendered.contains("details: /details last or /copy raw"));
        assert!(!rendered.contains("Command: sleep 60"));
        assert!(!rendered.contains("Cwd: /repo"));
        assert!(!rendered.contains("stderr: timed out"));
        assert!(!rendered.contains("Shell command finished and verification was recorded."));
    }

    #[test]
    fn policy_auto_created_actions_do_not_leave_pending_panel_noise() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "create package.json",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        pending_action.observe_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "package.json".to_string(),
            },
        )));

        assert_eq!(pending_action.render_body(), "none");
    }
}
