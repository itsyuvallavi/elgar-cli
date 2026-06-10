//! User approval commands for pending risky primitives.
//!
//! This module is the core approval boundary. UI and CLI surfaces call these
//! functions, but the pending request, approval state, execution, and logs stay
//! owned by core.

use std::fmt;

use crate::{harness::StructuredRequestKind, session::Session};

use super::{
    approval_logging::log_approval_decision, approved_bash::execute_approved_bash,
    approved_edit::execute_approved_edit, approved_write::execute_approved_write,
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
