//! Render verified evidence blocks for harness synthesis and error paths.

use crate::harness::{
    harness_loop::{evidence::timeline::render_verified_action_timeline, state::types::Evidence},
    PermissionDecision, ValidatedStructuredRequest,
};

/// Convert a failed execution into verified error evidence for synthesis.
pub(in crate::harness::harness_loop) fn error_evidence(label: String, error: &str) -> Evidence {
    let body = format!(
        "VERIFIED_EXECUTION_ERROR\nlabel: {label}\nerror: {error}\nfile_contents_read: false\n"
    );
    Evidence {
        label,
        bytes: body.len(),
        truncated: false,
        body,
    }
}

/// Convert a skipped no-op into verified evidence.
pub(in crate::harness::harness_loop) fn noop_evidence(label: String, reason: &str) -> Evidence {
    let body = format!(
        "VERIFIED_NOOP\ntool_target: {label}\nreason: {reason}\nexecution_performed: false\n"
    );
    Evidence {
        label,
        bytes: body.len(),
        truncated: false,
        body,
    }
}

/// Convert a blocked permission decision into verified evidence.
pub(in crate::harness::harness_loop) fn permission_evidence(
    label: String,
    request: &ValidatedStructuredRequest,
    decision: &PermissionDecision,
    approval_id: Option<&str>,
) -> Evidence {
    let approval_line = approval_id
        .map(|id| format!("approval_id: {id}\n"))
        .unwrap_or_default();
    let body = format!(
        "VERIFIED_PERMISSION_DECISION\ntool: {}\ndecision: {}\n{}reason: {}\napproval_required: {}\nexecution_performed: false\nvisible_instruction: Do not claim this operation ran. Tell the user approval is required before execution.\n",
        request.kind.as_str(),
        decision.kind.as_str(),
        approval_line,
        decision.reason.as_str(),
        if approval_id.is_some() { "true" } else { "false" }
    );
    Evidence {
        label,
        bytes: body.len(),
        truncated: false,
        body,
    }
}

/// Render verified evidence blocks for final synthesis.
pub(in crate::harness::harness_loop) fn render_evidence_for_synthesis(
    evidence: &[Evidence],
) -> String {
    if evidence.is_empty() {
        return "(none)".to_string();
    }

    let mut rendered = String::new();
    let timeline = render_verified_action_timeline(evidence);
    if !timeline.is_empty() {
        rendered.push_str(&timeline);
        rendered.push('\n');
    }
    for item in evidence {
        rendered.push_str("\n--- Verified Evidence: ");
        rendered.push_str(&item.label);
        rendered.push_str(" ---\n");
        rendered.push_str("truncated: ");
        rendered.push_str(if item.truncated { "true" } else { "false" });
        rendered.push('\n');
        rendered.push_str(&item.body);
        rendered.push('\n');
    }
    rendered
}
