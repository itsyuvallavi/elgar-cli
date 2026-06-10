//! Logging helpers for approval decisions and approved primitive execution.
//!
//! Approval flow owns when these logs are emitted. This module only formats the
//! system/session log events.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::{
    harness::PendingApproval,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(super) fn log_approval_decision(session: &Session, approval: &PendingApproval) {
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
    extra_metadata: Value,
) {
    let mut metadata = json!({
        "approval_id": approval.id,
        "tool": tool,
        "target_preview_chars": target_preview.chars().count(),
        "cwd": session.cwd,
        "started_unix_ms": unix_millis(),
    });
    merge_metadata(&mut metadata, extra_metadata);
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
    extra_metadata: Value,
) {
    let mut metadata = json!({
        "approval_id": approval.id,
        "tool": tool,
        "exit_code": exit_code,
        "duration_ms": duration_ms,
        "cwd": session.cwd,
    });
    merge_metadata(&mut metadata, extra_metadata);
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

fn merge_metadata(metadata: &mut Value, extra_metadata: Value) {
    let (Some(metadata), Some(extra_metadata)) =
        (metadata.as_object_mut(), extra_metadata.as_object())
    else {
        return;
    };
    for (key, value) in extra_metadata {
        metadata.insert(key.clone(), value.clone());
    }
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
