//! Permission and approval system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    harness::{PendingApproval, PermissionDecision, ValidatedStructuredRequest},
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_permission_decision(
    session: &Session,
    round_index: usize,
    request: &ValidatedStructuredRequest,
    decision: &PermissionDecision,
) {
    let metadata = json!({
        "round_index": round_index,
        "tool": request.kind.as_str(),
        "decision": decision.kind.as_str(),
        "reason": decision.reason.as_str(),
        "execution_allowed": decision.allows_execution()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_permission_decision",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_permission_decision", metadata);
}

pub(in crate::harness::harness_loop) fn log_harness_approval_requested(
    session: &Session,
    round_index: usize,
    approval: &PendingApproval,
) {
    let mut metadata = json!({
        "round_index": round_index,
        "approval_id": approval.id.as_str(),
        "tool": approval.tool.as_str(),
        "status": approval.status.as_str(),
        "reason": approval.reason.as_str(),
        "arguments_preview_chars": approval.arguments_preview.chars().count(),
        "execution_allowed": false
    });
    if let (Some(metadata), Some(target)) =
        (metadata.as_object_mut(), approval.target_preview.as_ref())
    {
        metadata.insert(
            "target_requested_path".to_string(),
            json!(target.requested_path),
        );
        metadata.insert(
            "target_resolved_preview_path".to_string(),
            json!(target.resolved_preview_path),
        );
        metadata.insert("target_is_absolute".to_string(), json!(target.is_absolute));
        metadata.insert("target_scope".to_string(), json!(target.scope.as_str()));
        metadata.insert("target_warning".to_string(), json!(target.warning));
    }
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_approval_requested",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_approval_requested", metadata);
}
