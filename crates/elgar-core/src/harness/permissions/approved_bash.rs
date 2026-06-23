//! Approved bash primitive execution.
//!
//! This module runs only the exact bash request stored in pending approval.

use std::{path::Path, process::Command, time::Instant};

use serde_json::json;

use crate::{harness::PendingApproval, session::Session};

use super::{
    approval_flow::{ApprovalCommandError, ApprovalCommandResult},
    approval_logging::{log_approved_execution_finished, log_approved_execution_started},
    approved_text::argument_text,
};

pub(super) fn execute_approved_bash(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let command = argument_text(&approval.request.arguments, "command")
        .ok_or(ApprovalCommandError::MissingArgument("command"))?;
    let requested_cwd = session.cwd.clone();
    let resolved_cwd = resolve_bash_cwd(&requested_cwd)?;

    let started = Instant::now();
    log_approved_execution_started(
        session,
        &approval,
        "bash",
        command,
        json!({
            "requested_cwd": requested_cwd.display().to_string(),
            "resolved_cwd": resolved_cwd.display().to_string(),
        }),
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&resolved_cwd)
        .output()
        .map_err(|error| ApprovalCommandError::ExecutionFailed(error.to_string()))?;

    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log_approved_execution_finished(
        session,
        &approval,
        "bash",
        exit_code,
        duration_ms,
        json!({
            "requested_cwd": requested_cwd.display().to_string(),
            "resolved_cwd": resolved_cwd.display().to_string(),
        }),
    );

    let message = render_bash_execution_message(
        &approval,
        command,
        &requested_cwd.display().to_string(),
        &resolved_cwd.display().to_string(),
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

fn resolve_bash_cwd(cwd: &Path) -> Result<std::path::PathBuf, ApprovalCommandError> {
    let resolved = cwd.canonicalize().map_err(|error| {
        ApprovalCommandError::ExecutionFailed(format!(
            "approved bash cwd is unreadable: {} ({error})",
            cwd.display()
        ))
    })?;
    if !resolved.is_dir() {
        return Err(ApprovalCommandError::ExecutionFailed(format!(
            "approved bash cwd is not a directory: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

#[allow(clippy::too_many_arguments)]
fn render_bash_execution_message(
    approval: &PendingApproval,
    command: &str,
    requested_cwd: &str,
    resolved_cwd: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
    stdout: &str,
    stderr: &str,
) -> String {
    format!(
        "VERIFIED_BASH_EXECUTION\napproval_id: {}\ncommand: {}\nrequested_cwd: {}\nresolved_cwd: {}\nexit_code: {}\nduration_ms: {}\nstdout:\n{}stderr:\n{}",
        approval.id,
        command,
        requested_cwd,
        resolved_cwd,
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        duration_ms,
        stdout,
        stderr
    )
}
