//! Verified-evidence system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    harness::harness_loop::{
        evidence::timeline::VerifiedActionTimelineStats, state::types::Evidence,
    },
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_loop_evidence(
    session: &Session,
    round_index: usize,
    evidence: &Evidence,
) {
    let metadata = json!({
        "round_index": round_index,
        "evidence_label": evidence.label,
        "evidence_bytes": evidence.bytes,
        "truncated": evidence.truncated
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_loop_evidence_collected",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_tool_result_verified", metadata);
}

pub(in crate::harness::harness_loop) fn log_verified_action_timeline_appended(
    session: &Session,
    round_index: usize,
    stats: VerifiedActionTimelineStats,
) {
    let metadata = verified_action_timeline_metadata(round_index, stats);
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_verified_action_timeline_appended",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_verified_action_timeline_appended", metadata);
}

fn verified_action_timeline_metadata(
    round_index: usize,
    stats: VerifiedActionTimelineStats,
) -> serde_json::Value {
    json!({
        "round_index": round_index,
        "verified_action_timeline_appended": true,
        "timeline_action_count": stats.action_count,
        "timeline_rendered_action_count": stats.rendered_action_count,
        "timeline_failed_command_count": stats.failed_command_count
    })
}

#[cfg(test)]
mod tests {
    use crate::harness::harness_loop::evidence::timeline::VerifiedActionTimelineStats;

    use super::verified_action_timeline_metadata;

    #[test]
    fn timeline_log_metadata_is_compact() {
        let metadata = verified_action_timeline_metadata(
            7,
            VerifiedActionTimelineStats {
                action_count: 4,
                rendered_action_count: 4,
                failed_command_count: 1,
            },
        );

        assert_eq!(metadata["round_index"], 7);
        assert_eq!(metadata["verified_action_timeline_appended"], true);
        assert_eq!(metadata["timeline_action_count"], 4);
        assert_eq!(metadata["timeline_rendered_action_count"], 4);
        assert_eq!(metadata["timeline_failed_command_count"], 1);
        assert!(metadata.get("timeline_body").is_none());
    }
}
