//! Approved write primitive execution.
//!
//! This module creates or overwrites one file only after the exact pending
//! approval request has been approved.

use std::{fs, time::Instant};

use crate::{harness::PendingApproval, session::Session};

use super::{
    approval_flow::{ApprovalCommandError, ApprovalCommandResult},
    approval_logging::{log_approved_execution_finished, log_approved_execution_started},
    approved_paths::resolve_write_target,
    approved_text::{argument_raw_text, argument_text},
};

pub(super) fn execute_approved_write(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let path = argument_text(&approval.request.arguments, "path")
        .ok_or(ApprovalCommandError::MissingArgument("path"))?;
    let content = argument_raw_text(&approval.request.arguments, "content")
        .ok_or(ApprovalCommandError::MissingArgument("content"))?;
    let target = resolve_write_target(&session.cwd, path)
        .map_err(|error| ApprovalCommandError::PathRejected(error.to_string()))?;

    let started = Instant::now();
    log_approved_execution_started(session, &approval, "write", path, serde_json::json!({}));
    fs::write(&target, content)
        .map_err(|error| ApprovalCommandError::ExecutionFailed(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_approved_execution_finished(
        session,
        &approval,
        "write",
        Some(0),
        duration_ms,
        serde_json::json!({}),
    );

    Ok(ApprovalCommandResult {
        approval_id: approval.id.clone(),
        status: approval.status.as_str(),
        message: format!(
            "VERIFIED_WRITE_EXECUTION\napproval_id: {}\npath: {}\nresolved_path: {}\nbytes_written: {}\nduration_ms: {}\n",
            approval.id,
            path,
            target.display(),
            content.len(),
            duration_ms
        ),
    })
}
