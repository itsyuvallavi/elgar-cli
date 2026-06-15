//! Verified evidence helpers for the primitive harness loop.
//!
//! Evidence here only comes from Rust collectors. Provider prose is never
//! promoted into verified evidence.

use std::{fs, process::Command, time::Instant};

use serde_json::json;

use crate::{
    harness::{
        collect_directory_summary, collect_find_matches, collect_grep_matches,
        collect_project_file, resolve_write_target, DirectoryOptions, FindOptions, GrepOptions,
        ModelChoiceTurnError, ProjectFileOptions, StructuredRequestKind,
        ValidatedStructuredRequest,
    },
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

use crate::harness::harness_loop::{
    evidence::{
        keys::normalize_evidence_path,
        request_args::{request_path, request_pattern, request_query},
    },
    state::{listing_memory::DirectoryListingMemory, types::Evidence},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct ExecutedEvidence {
    pub evidence: Evidence,
    pub directory_listing: Option<DirectoryListingMemory>,
}

/// Execute one validated primitive request and return verified evidence.
pub(in crate::harness::harness_loop) fn execute_primitive_request(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<ExecutedEvidence, ModelChoiceTurnError> {
    match request.kind {
        StructuredRequestKind::Read => {
            let path = request_path(request).unwrap_or_default();
            let label_path = normalize_evidence_path(path);
            let snapshot = collect_project_file(&session.cwd, path, ProjectFileOptions::default())
                .map_err(|error| ModelChoiceTurnError::ProjectFile(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("read:{label_path}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Ls => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let snapshot =
                collect_directory_summary(&session.cwd, path, DirectoryOptions::default())
                    .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let directory_listing =
                DirectoryListingMemory::from_snapshot(label_path.clone(), &snapshot);
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("ls:{label_path}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated || snapshot.count_truncated,
                    body,
                },
                directory_listing: Some(directory_listing),
            })
        }
        StructuredRequestKind::Find => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let pattern = request_pattern(request).unwrap_or_default();
            let snapshot =
                collect_find_matches(&session.cwd, path, pattern, FindOptions::default())
                    .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("find:{label_path}:{pattern}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Grep => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let query = request_query(request).unwrap_or_default();
            let snapshot = collect_grep_matches(&session.cwd, path, query, GrepOptions::default())
                .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("grep:{label_path}:{query}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Write => execute_policy_write(session, request),
        StructuredRequestKind::Bash => execute_policy_bash(session, request),
        StructuredRequestKind::Edit => execute_policy_edit(session, request),
        StructuredRequestKind::McpCall => {
            let evidence = super::mcp::execute_mcp_call_request(session, request)?;
            Ok(ExecutedEvidence {
                evidence,
                directory_listing: None,
            })
        }
    }
}

fn execute_policy_bash(
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
    let started = Instant::now();
    log_policy_bash_started(session, command, &resolved_cwd.display().to_string());
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
    );

    let body = format!(
        "VERIFIED_BASH_EXECUTION\napproval_source: full_access\napproval_required: false\nauto_approved: true\ncommand: {}\nrequested_cwd: {}\nresolved_cwd: {}\nexit_code: {}\nduration_ms: {}\nstdout:\n{}stderr:\n{}",
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

fn execute_policy_edit(
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

    let started = Instant::now();
    log_policy_edit_started(session, path);
    let updated = original.replacen(old_text, new_text, 1);
    fs::write(&target, updated)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_policy_edit_finished(session, path, duration_ms);
    let body = format!(
        "VERIFIED_EDIT_EXECUTION\napproval_source: full_access\napproval_required: false\nauto_approved: true\npath: {}\nresolved_path: {}\nold_text_bytes: {}\nnew_text_bytes: {}\nreplacements: 1\nduration_ms: {}\n",
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

fn execute_policy_write(
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
    let started = Instant::now();
    log_policy_write_started(session, path, approval_source);
    fs::write(&target, content)
        .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
    let duration_ms = started.elapsed().as_millis() as u64;
    log_policy_write_finished(session, path, duration_ms, approval_source);

    let label = format!("write:{}", normalize_evidence_path(path));
    let body = format!(
        "VERIFIED_WRITE_EXECUTION\napproval_source: {}\napproval_required: false\nauto_approved: true\npath: {}\nresolved_path: {}\nbytes_written: {}\nduration_ms: {}\n",
        approval_source,
        path,
        target.display(),
        content.len(),
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

fn log_policy_write_started(session: &Session, path: &str, approval_source: &str) {
    let metadata = json!({
        "tool": "write",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
        "cwd": session.cwd,
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "execute_policy_write",
            "harness_policy_write_execution_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_write_execution_started", metadata);
}

fn log_policy_write_finished(
    session: &Session,
    path: &str,
    duration_ms: u64,
    approval_source: &str,
) {
    let metadata = json!({
        "tool": "write",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
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
            "execute_policy_write",
            "harness_policy_write_execution_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_write_execution_finished", metadata);
}

fn log_policy_bash_started(session: &Session, command: &str, resolved_cwd: &str) {
    let metadata = json!({
        "tool": "bash",
        "approval_source": "full_access",
        "approval_required": false,
        "command": command,
        "requested_cwd": session.cwd,
        "resolved_cwd": resolved_cwd,
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "execute_policy_bash",
            "harness_policy_bash_execution_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_bash_execution_started", metadata);
}

fn log_policy_bash_finished(
    session: &Session,
    command: &str,
    resolved_cwd: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
) {
    let metadata = json!({
        "tool": "bash",
        "approval_source": "full_access",
        "approval_required": false,
        "command": command,
        "requested_cwd": session.cwd,
        "resolved_cwd": resolved_cwd,
        "exit_code": exit_code,
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "execute_policy_bash",
            "harness_policy_bash_execution_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_bash_execution_finished", metadata);
}

fn log_policy_edit_started(session: &Session, path: &str) {
    let metadata = json!({
        "tool": "edit",
        "approval_source": "full_access",
        "approval_required": false,
        "path": path,
        "cwd": session.cwd,
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "execute_policy_edit",
            "harness_policy_edit_execution_started",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_edit_execution_started", metadata);
}

fn log_policy_edit_finished(session: &Session, path: &str, duration_ms: u64) {
    let metadata = json!({
        "tool": "edit",
        "approval_source": "full_access",
        "approval_required": false,
        "path": path,
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
            "execute_policy_edit",
            "harness_policy_edit_execution_finished",
        )
        .with_duration_ms(duration_ms)
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_policy_edit_execution_finished", metadata);
}
