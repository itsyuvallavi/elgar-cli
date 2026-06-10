//! Approved bash primitive execution.
//!
//! This module runs only the exact bash request stored in pending approval.

use std::{process::Command, time::Instant};

use crate::{harness::PendingApproval, session::Session};

use super::{
    approval_flow::{
        log_approved_execution_finished, log_approved_execution_started, ApprovalCommandError,
        ApprovalCommandResult,
    },
    approved_text::argument_text,
};

pub(super) fn execute_approved_bash(
    session: &mut Session,
    approval: PendingApproval,
) -> Result<ApprovalCommandResult, ApprovalCommandError> {
    let command = argument_text(&approval.request.arguments, "command")
        .ok_or(ApprovalCommandError::MissingArgument("command"))?;

    let started = Instant::now();
    log_approved_execution_started(session, &approval, "bash", command);
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&session.cwd)
        .output()
        .map_err(|error| ApprovalCommandError::ExecutionFailed(error.to_string()))?;

    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log_approved_execution_finished(session, &approval, "bash", exit_code, duration_ms);

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
