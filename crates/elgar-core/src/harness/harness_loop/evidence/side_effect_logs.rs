//! System and session logging for trusted side-effect execution.
//!
//! Side-effect executors call these helpers after permission policy has already
//! allowed immediate execution.

use serde_json::{json, Value};

use crate::{
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(super) fn log_policy_write_started(session: &Session, path: &str, approval_source: &str) {
    let metadata = json!({
        "tool": "write",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
        "cwd": session.cwd,
    });
    append_policy_log(
        session,
        "execute_policy_write",
        "harness_policy_write_execution_started",
        None,
        metadata,
    );
}

pub(super) fn log_policy_write_finished(
    session: &Session,
    path: &str,
    duration_ms: u64,
    approval_source: &str,
    outcome_metadata: Value,
) {
    let mut metadata = json!({
        "tool": "write",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
        "duration_ms": duration_ms,
        "cwd": session.cwd,
    });
    merge_json_object(&mut metadata, outcome_metadata);
    append_policy_log(
        session,
        "execute_policy_write",
        "harness_policy_write_execution_finished",
        Some(duration_ms),
        metadata,
    );
}

pub(super) fn log_policy_bash_started(
    session: &Session,
    command: &str,
    resolved_cwd: &str,
    approval_source: &str,
) {
    let metadata = json!({
        "tool": "bash",
        "approval_source": approval_source,
        "approval_required": false,
        "command": command,
        "requested_cwd": session.cwd,
        "resolved_cwd": resolved_cwd,
    });
    append_policy_log(
        session,
        "execute_policy_bash",
        "harness_policy_bash_execution_started",
        None,
        metadata,
    );
}

pub(super) fn log_policy_bash_finished(
    session: &Session,
    command: &str,
    resolved_cwd: &str,
    exit_code: Option<i32>,
    duration_ms: u64,
    approval_source: &str,
) {
    let metadata = json!({
        "tool": "bash",
        "approval_source": approval_source,
        "approval_required": false,
        "command": command,
        "requested_cwd": session.cwd,
        "resolved_cwd": resolved_cwd,
        "exit_code": exit_code,
    });
    append_policy_log(
        session,
        "execute_policy_bash",
        "harness_policy_bash_execution_finished",
        Some(duration_ms),
        metadata,
    );
}

pub(super) fn log_policy_edit_started(session: &Session, path: &str, approval_source: &str) {
    let metadata = json!({
        "tool": "edit",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
        "cwd": session.cwd,
    });
    append_policy_log(
        session,
        "execute_policy_edit",
        "harness_policy_edit_execution_started",
        None,
        metadata,
    );
}

pub(super) fn log_policy_edit_finished(
    session: &Session,
    path: &str,
    duration_ms: u64,
    approval_source: &str,
) {
    let metadata = json!({
        "tool": "edit",
        "approval_source": approval_source,
        "approval_required": false,
        "path": path,
        "duration_ms": duration_ms,
        "cwd": session.cwd,
    });
    append_policy_log(
        session,
        "execute_policy_edit",
        "harness_policy_edit_execution_finished",
        Some(duration_ms),
        metadata,
    );
}

fn append_policy_log(
    session: &Session,
    function_name: &'static str,
    event_name: &'static str,
    duration_ms: Option<u64>,
    metadata: Value,
) {
    let mut input = LogInput::new(
        session.next_turn_id(),
        LogPhase::Runtime,
        file!(),
        function_name,
        event_name,
    )
    .with_metadata(metadata.clone());
    if let Some(duration_ms) = duration_ms {
        input = input.with_duration_ms(duration_ms);
    }
    let _ = append_log_event(&session.project_root, &session.id, input);
    session.log_harness_event(event_name, metadata);
}

fn merge_json_object(target: &mut Value, source: Value) {
    let (Some(target), Some(source)) = (target.as_object_mut(), source.as_object()) else {
        return;
    };
    for (key, value) in source {
        target.insert(key.clone(), value.clone());
    }
}
