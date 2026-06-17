//! Verified side-effect execution for trusted harness modes.
//!
//! These helpers run validated `write`, `edit`, and `bash` requests only after
//! permission policy has allowed immediate execution.

use std::{fs, process::Command, time::Instant};

use crate::{
    harness::{
        harness_loop::{
            evidence::{
                execution::ExecutedEvidence,
                keys::normalize_evidence_path,
                request_args::request_path,
                side_effect_logs::{
                    log_policy_bash_finished, log_policy_bash_started, log_policy_edit_finished,
                    log_policy_edit_started, log_policy_write_finished, log_policy_write_started,
                },
            },
            state::types::Evidence,
        },
        resolve_write_target, ModelChoiceTurnError, ValidatedStructuredRequest, WriteOutcome,
    },
    session::Session,
};

pub(super) fn execute_policy_bash(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<ExecutedEvidence, ModelChoiceTurnError> {
    let command = request
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.get("command"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let resolved_cwd = session
        .cwd
        .canonicalize()
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let approval_source = session.permission_mode().as_str();
    let started = Instant::now();
    log_policy_bash_started(
        session,
        command,
        &resolved_cwd.display().to_string(),
        approval_source,
    );
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(&resolved_cwd)
        .output()
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    let exit_code = output.status.code();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    log_policy_bash_finished(
        session,
        command,
        &resolved_cwd.display().to_string(),
        exit_code,
        duration_ms,
        approval_source,
    );

    let body = format!(
        "VERIFIED_BASH_EXECUTION\napproval_source: {}\napproval_required: false\nauto_approved: true\ncommand: {}\nrequested_cwd: {}\nresolved_cwd: {}\nexit_code: {}\nduration_ms: {}\nstdout:\n{}stderr:\n{}",
        approval_source,
        command,
        session.cwd.display(),
        resolved_cwd.display(),
        exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        duration_ms,
        stdout,
        stderr
    );
    Ok(ExecutedEvidence {
        evidence: Evidence {
            label: format!("bash:{}", normalize_evidence_path(command)),
            bytes: body.len(),
            truncated: false,
            body,
        },
        directory_listing: None,
    })
}

pub(super) fn execute_policy_edit(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<ExecutedEvidence, ModelChoiceTurnError> {
    let arguments = request.arguments.as_ref();
    let path = request_path(request).unwrap_or_default();
    let old_text = arguments
        .and_then(|arguments| arguments.get("old_text"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let new_text = arguments
        .and_then(|arguments| arguments.get("new_text"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if old_text.is_empty() {
        return Err(ModelChoiceTurnError::ProjectContext(
            "old_text must not be empty".to_string(),
        ));
    }

    let target = resolve_write_target(&session.cwd, path)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let original = fs::read_to_string(&target)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let matches = original.match_indices(old_text).count();
    if matches == 0 {
        return Err(ModelChoiceTurnError::ProjectContext(
            "old_text was not found".to_string(),
        ));
    }
    if matches > 1 {
        return Err(ModelChoiceTurnError::ProjectContext(format!(
            "old_text matched {matches} times"
        )));
    }

    let approval_source = session.permission_mode().as_str();
    let started = Instant::now();
    log_policy_edit_started(session, path, approval_source);
    let updated = original.replacen(old_text, new_text, 1);
    fs::write(&target, updated)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_policy_edit_finished(session, path, duration_ms, approval_source);
    let body = format!(
        "VERIFIED_EDIT_EXECUTION\napproval_source: {}\napproval_required: false\nauto_approved: true\npath: {}\nresolved_path: {}\nold_text_bytes: {}\nnew_text_bytes: {}\nreplacements: 1\nduration_ms: {}\n",
        approval_source,
        path,
        target.display(),
        old_text.len(),
        new_text.len(),
        duration_ms
    );
    Ok(ExecutedEvidence {
        evidence: Evidence {
            label: format!("edit:{}", normalize_evidence_path(path)),
            bytes: body.len(),
            truncated: false,
            body,
        },
        directory_listing: None,
    })
}

pub(super) fn execute_policy_write(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<ExecutedEvidence, ModelChoiceTurnError> {
    let path = request_path(request).unwrap_or_default();
    let content = request
        .arguments
        .as_ref()
        .and_then(|arguments| arguments.get("content"))
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let target = resolve_write_target(&session.cwd, path)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let approval_source = session.permission_mode().as_str();
    let outcome = WriteOutcome::inspect(&target, content);
    let started = Instant::now();
    log_policy_write_started(session, path, approval_source);
    fs::write(&target, content)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_policy_write_finished(
        session,
        path,
        duration_ms,
        approval_source,
        outcome.metadata(),
    );

    let label = format!("write:{}", normalize_evidence_path(path));
    let body = format!(
        "VERIFIED_WRITE_EXECUTION\napproval_source: {}\napproval_required: false\nauto_approved: true\npath: {}\nresolved_path: {}\nbytes_written: {}\n{}duration_ms: {}\n",
        approval_source,
        path,
        target.display(),
        content.len(),
        outcome.raw_lines(),
        duration_ms
    );
    Ok(ExecutedEvidence {
        evidence: Evidence {
            label,
            bytes: body.len(),
            truncated: false,
            body,
        },
        directory_listing: None,
    })
}
