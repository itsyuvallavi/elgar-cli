//! Approved edit primitive execution.
//!
//! The first edit executor supports exact text replacement only. It rejects
//! missing or repeated old text to avoid broad unintended changes.

use std::{fs, time::Instant};

use crate::{harness::PendingApproval, session::Session};

use super::{
    approval_flow::{ApprovalCommandError, ApprovalCommandResult},
    approval_logging::{log_approved_execution_finished, log_approved_execution_started},
    approved_paths::resolve_existing_file_target,
    approved_text::{argument_raw_text, argument_text},
};

pub(super) fn execute_approved_edit(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let path = argument_text(&approval.request.arguments, "path")
        .ok_or(ApprovalCommandError::MissingArgument("path"))?;
    let old_text = argument_raw_text(&approval.request.arguments, "old_text")
        .ok_or(ApprovalCommandError::MissingArgument("old_text"))?;
    let new_text = argument_raw_text(&approval.request.arguments, "new_text")
        .ok_or(ApprovalCommandError::MissingArgument("new_text"))?;
    if old_text.is_empty() {
        return Err(ApprovalCommandError::InvalidEdit(
            "old_text must not be empty".to_string(),
        ));
    }

    let target = resolve_existing_file_target(&session.cwd, path)
        .map_err(|error| ApprovalCommandError::PathRejected(error.to_string()))?;
    let original = fs::read_to_string(&target)
        .map_err(|error| ApprovalCommandError::ExecutionFailed(error.to_string()))?;
    let matches = original.match_indices(old_text).count();
    if matches == 0 {
        return Err(ApprovalCommandError::InvalidEdit(
            "old_text was not found".to_string(),
        ));
    }
    if matches > 1 {
        return Err(ApprovalCommandError::InvalidEdit(format!(
            "old_text matched {matches} times"
        )));
    }

    let started = Instant::now();
    log_approved_execution_started(session, &approval, "edit", path, serde_json::json!({}));
    let updated = original.replacen(old_text, new_text, 1);
    fs::write(&target, updated)
        .map_err(|error| ApprovalCommandError::ExecutionFailed(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_approved_execution_finished(
        session,
        &approval,
        "edit",
        Some(0),
        duration_ms,
        serde_json::json!({}),
    );

    Ok(ApprovalCommandResult {
        approval_id: approval.id.clone(),
        status: approval.status.as_str(),
        message: format!(
            "VERIFIED_EDIT_EXECUTION\napproval_id: {}\npath: {}\nresolved_path: {}\nold_text_bytes: {}\nnew_text_bytes: {}\nreplacements: 1\nduration_ms: {}\n",
            approval.id,
            path,
            target.display(),
            old_text.len(),
            new_text.len(),
            duration_ms
        ),
    })
}
