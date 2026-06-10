//! User approval commands for pending risky primitives.
//!
//! This module is the core approval boundary. UI and CLI surfaces call these
//! functions, but the pending request, approval state, execution, and logs stay
//! owned by core.

use std::{
    fmt,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};

use crate::{
    harness::{PendingApproval, StructuredRequestKind},
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalCommandResult {
    pub approval_id: String,
    pub status: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalCommandError {
    NoPendingApproval,
    UnsupportedApprovedTool(String),
    MissingCommand,
    ShellFailed(String),
}

impl fmt::Display for ApprovalCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPendingApproval => write!(formatter, "No pending approval."),
            Self::UnsupportedApprovedTool(tool) => {
                write!(
                    formatter,
                    "Approved primitive `{tool}` is not executable yet."
                )
            }
            Self::MissingCommand => write!(formatter, "Approved bash request is missing command."),
            Self::ShellFailed(error) => {
                write!(formatter, "Approved bash execution failed: {error}")
            }
        }
    }
}

/// Deny and clear the current pending approval.
pub fn deny_pending_approval(
    session: &mut Session,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let Some(approval) = session.take_pending_approval() else {
        return Err(ApprovalCommandError::NoPendingApproval);
    };
    let denied = approval.deny();
    log_approval_decision(session, &denied);

    Ok(ApprovalCommandResult {
        approval_id: denied.id.clone(),
        status: denied.status.as_str(),
        message: format!("Denied {} for `{}`.", denied.id, denied.tool),
    })
}

/// Approve and execute the current pending approval when its primitive is ready.
pub fn approve_pending_approval(
    session: &mut Session,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let Some(approval) = session.take_pending_approval() else {
        return Err(ApprovalCommandError::NoPendingApproval);
    };
    let approved = approval.approve();
    log_approval_decision(session, &approved);

    match approved.request.kind {
        StructuredRequestKind::Bash => execute_approved_bash(session, approved),
        StructuredRequestKind::Write | StructuredRequestKind::Edit => {
            let tool = approved.tool.clone();
            session.restore_pending_approval(approved);
            Err(ApprovalCommandError::UnsupportedApprovedTool(tool))
        }
        StructuredRequestKind::Read
        | StructuredRequestKind::Ls
        | StructuredRequestKind::Find
        | StructuredRequestKind::Grep => {
            let tool = approved.tool.clone();
            Err(ApprovalCommandError::UnsupportedApprovedTool(tool))
        }
    }
}

fn execute_approved_bash(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let command = approval
        .request
        .arguments
        .as_ref()
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(ApprovalCommandError::MissingCommand)?;

    let started = Instant::now();
    log_bash_execution_started(session, &approval, command);
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&session.cwd)
        .output()
        .map_err(|error| ApprovalCommandError::ShellFailed(error.to_string()))?;

    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log_bash_execution_finished(session, &approval, command, exit_code, duration_ms);

    let message = render_bash_execution_message(
        &approval,
        command,
        &session.cwd.display().to_string(),
        exit_code,
        duration_ms,
        &stdout,
        &stderr,
    );

    Ok(ApprovalCommandResult {
        approval_id: approval.id.clone(),
        status: approval.status.as_str(),
        message,
    })
}

fn render_bash_execution_message(
    approval: &PendingApproval,
    command: &str,
    cwd: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout: &str,
    stderr: &str,
) -> String {
    format!(
        "VERIFIED_BASH_EXECUTION\napproval_id: {}\ncommand: {}\ncwd: {}\nexit_code: {}\nduration_ms: {}\nstdout:\n{}stderr:\n{}",
        approval.id,
        command,
        cwd,
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        duration_ms,
        stdout,
        stderr
    )
}

fn log_approval_decision(session: &Session, approval: &PendingApproval) {
    let metadata = json!({
        "approval_id": approval.id,
        "tool": approval.tool,
        "status": approval.status.as_str(),
        "arguments_preview_chars": approval.arguments_preview.chars().count(),
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "log_approval_decision",
            "harness_approval_decision",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_approval_decision", metadata);
}

fn log_bash_execution_started(session: &Session, approval: &PendingApproval, command: &str) {
    let metadata = json!({
        "approval_id": approval.id,
        "tool": "bash",
        "command_chars": command.chars().count(),
        "cwd": session.cwd,
        "started_unix_ms": unix_millis(),
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "log_bash_execution_started",
            "harness_bash_execution_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_bash_execution_started", metadata);
}

fn log_bash_execution_finished(
    session: &Session,
    approval: &PendingApproval,
    command: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
) {
    let metadata = json!({
        "approval_id": approval.id,
        "tool": "bash",
        "command_chars": command.chars().count(),
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "cwd": session.cwd,
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "log_bash_execution_finished",
            "harness_bash_execution_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_bash_execution_finished", metadata);
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::{
        harness::{
            approve_pending_approval, deny_pending_approval, PendingApproval,
            StructuredRequestKind, ValidatedStructuredRequest,
        },
        session::Session,
    };

    #[test]
    fn deny_pending_approval_clears_session_slot() {
        let root = std::env::temp_dir().join(format!(
            "elgar-deny-pending-approval-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("deny-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &bash_request("echo no"),
            "needs approval",
        ));

        let result = deny_pending_approval(&mut session).unwrap();

        assert_eq!(result.approval_id, "approval-1");
        assert_eq!(result.status, "denied");
        assert!(session.pending_approval().is_none());
    }

    #[test]
    fn approve_pending_bash_executes_stored_command() {
        let root =
            std::env::temp_dir().join(format!("elgar-approve-pending-bash-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("approve-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &bash_request("echo approved-bash"),
            "needs approval",
        ));

        let result = approve_pending_approval(&mut session).unwrap();

        assert_eq!(result.approval_id, "approval-1");
        assert_eq!(result.status, "approved");
        assert!(result.message.contains("VERIFIED_BASH_EXECUTION"));
        assert!(result.message.contains("approved-bash"));
        assert!(session.pending_approval().is_none());
    }

    fn bash_request(command: &str) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Bash,
            reason: "test".to_string(),
            arguments: Some(json!({ "command": command })),
        }
    }
}
