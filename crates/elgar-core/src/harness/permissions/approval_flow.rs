//! User approval commands for pending risky primitives.
//!
//! This module is the core approval boundary. UI and CLI surfaces call these
//! functions, but the pending request, approval state, execution, and logs stay
//! owned by core.

use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;

use crate::{
    harness::{PendingApproval, StructuredRequestKind},
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

use super::{
    approved_bash::execute_approved_bash, approved_edit::execute_approved_edit,
    approved_write::execute_approved_write,
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
    MissingArgument(&'static str),
    PathRejected(String),
    InvalidEdit(String),
    ExecutionFailed(String),
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
            Self::MissingArgument(name) => {
                write!(formatter, "Approved request is missing `{name}`.")
            }
            Self::PathRejected(error) => write!(formatter, "Approved path rejected: {error}"),
            Self::InvalidEdit(error) => write!(formatter, "Approved edit rejected: {error}"),
            Self::ExecutionFailed(error) => write!(formatter, "Approved execution failed: {error}"),
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
        StructuredRequestKind::Write => execute_approved_write(session, approved),
        StructuredRequestKind::Edit => execute_approved_edit(session, approved),
        StructuredRequestKind::Read
        | StructuredRequestKind::Ls
        | StructuredRequestKind::Find
        | StructuredRequestKind::Grep => {
            let tool = approved.tool.clone();
            Err(ApprovalCommandError::UnsupportedApprovedTool(tool))
        }
    }
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

pub(super) fn log_approved_execution_started(
    session: &Session,
    approval: &PendingApproval,
    tool: &'static str,
    target_preview: &str,
) {
    let metadata = json!({
        "approval_id": approval.id,
        "tool": tool,
        "target_preview_chars": target_preview.chars().count(),
        "cwd": session.cwd,
        "started_unix_ms": unix_millis(),
    });
    let summary = execution_started_summary(tool);
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "log_approved_execution_started",
            summary,
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event(summary, metadata);
}

pub(super) fn log_approved_execution_finished(
    session: &Session,
    approval: &PendingApproval,
    tool: &'static str,
    exit_code: Option<i32>,
    duration_ms: u64,
) {
    let metadata = json!({
        "approval_id": approval.id,
        "tool": tool,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "cwd": session.cwd,
    });
    let summary = execution_finished_summary(tool);
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "log_approved_execution_finished",
            summary,
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event(summary, metadata);
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn execution_started_summary(tool: &str) -> &'static str {
    match tool {
        "bash" => "harness_bash_execution_started",
        "write" => "harness_write_execution_started",
        "edit" => "harness_edit_execution_started",
        _ => "harness_approved_execution_started",
    }
}

fn execution_finished_summary(tool: &str) -> &'static str {
    match tool {
        "bash" => "harness_bash_execution_finished",
        "write" => "harness_write_execution_finished",
        "edit" => "harness_edit_execution_finished",
        _ => "harness_approved_execution_finished",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::{
        harness::{
            approve_pending_approval, deny_pending_approval, ApprovalCommandError, PendingApproval,
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

    #[test]
    fn approve_pending_write_creates_file() {
        let root = std::env::temp_dir().join(format!(
            "elgar-approve-pending-write-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("write-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &write_request("nested/demo.txt", "hello\n"),
            "needs approval",
        ));

        let result = approve_pending_approval(&mut session).unwrap();

        assert_eq!(result.status, "approved");
        assert!(result.message.contains("VERIFIED_WRITE_EXECUTION"));
        assert_eq!(
            fs::read_to_string(root.join("nested/demo.txt")).unwrap(),
            "hello\n"
        );
        assert!(session.pending_approval().is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn approve_pending_write_overwrites_file() {
        let root = std::env::temp_dir().join(format!(
            "elgar-approve-pending-write-overwrite-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "old").unwrap();
        let mut session = Session::new("write-overwrite-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &write_request("demo.txt", "new"),
            "needs approval",
        ));

        approve_pending_approval(&mut session).unwrap();

        assert_eq!(fs::read_to_string(root.join("demo.txt")).unwrap(), "new");
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn approve_pending_write_rejects_symlink_target() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!(
            "elgar-approve-pending-write-symlink-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("real.txt"), "real").unwrap();
        symlink(root.join("real.txt"), root.join("link.txt")).unwrap();
        let mut session = Session::new("write-symlink-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &write_request("link.txt", "no"),
            "needs approval",
        ));

        let error = approve_pending_approval(&mut session).unwrap_err();

        assert!(matches!(error, ApprovalCommandError::PathRejected(_)));
        assert_eq!(fs::read_to_string(root.join("real.txt")).unwrap(), "real");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn approve_pending_edit_replaces_exact_text() {
        let root =
            std::env::temp_dir().join(format!("elgar-approve-pending-edit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "alpha beta gamma").unwrap();
        let mut session = Session::new("edit-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &edit_request("demo.txt", "beta", "BETA"),
            "needs approval",
        ));

        let result = approve_pending_approval(&mut session).unwrap();

        assert!(result.message.contains("VERIFIED_EDIT_EXECUTION"));
        assert_eq!(
            fs::read_to_string(root.join("demo.txt")).unwrap(),
            "alpha BETA gamma"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn approve_pending_edit_rejects_missing_old_text() {
        let root = std::env::temp_dir().join(format!(
            "elgar-approve-pending-edit-missing-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "alpha beta gamma").unwrap();
        let mut session = Session::new("edit-missing-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &edit_request("demo.txt", "delta", "DELTA"),
            "needs approval",
        ));

        let error = approve_pending_approval(&mut session).unwrap_err();

        assert!(matches!(error, ApprovalCommandError::InvalidEdit(_)));
        assert_eq!(
            fs::read_to_string(root.join("demo.txt")).unwrap(),
            "alpha beta gamma"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn approve_pending_edit_rejects_multiple_old_text_matches() {
        let root = std::env::temp_dir().join(format!(
            "elgar-approve-pending-edit-multiple-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("demo.txt"), "alpha beta beta").unwrap();
        let mut session = Session::new("edit-multiple-session", &root, &root);
        session.set_pending_approval(PendingApproval::from_request(
            "approval-1",
            &edit_request("demo.txt", "beta", "BETA"),
            "needs approval",
        ));

        let error = approve_pending_approval(&mut session).unwrap_err();

        assert!(matches!(error, ApprovalCommandError::InvalidEdit(_)));
        assert_eq!(
            fs::read_to_string(root.join("demo.txt")).unwrap(),
            "alpha beta beta"
        );
        let _ = fs::remove_dir_all(root);
    }

    fn bash_request(command: &str) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Bash,
            reason: "test".to_string(),
            arguments: Some(json!({ "command": command })),
        }
    }

    fn write_request(path: &str, content: &str) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "test".to_string(),
            arguments: Some(json!({ "path": path, "content": content })),
        }
    }

    fn edit_request(path: &str, old_text: &str, new_text: &str) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind: StructuredRequestKind::Edit,
            reason: "test".to_string(),
            arguments: Some(json!({
                "path": path,
                "old_text": old_text,
                "new_text": new_text
            })),
        }
    }
}
