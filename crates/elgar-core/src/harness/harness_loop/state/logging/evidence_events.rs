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
    let mut metadata = json!({
        "round_index": round_index,
        "evidence_label": evidence.label,
        "evidence_bytes": evidence.bytes,
        "truncated": evidence.truncated
    });
    add_write_outcome_metadata(&mut metadata, evidence);
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

fn add_write_outcome_metadata(metadata: &mut serde_json::Value, evidence: &Evidence) {
    if let Some(value) = evidence_field(&evidence.body, "existed_before").and_then(parse_bool) {
        metadata["existed_before"] = json!(value);
    }
    if let Some(value) = evidence_field(&evidence.body, "content_changed") {
        metadata["content_changed"] = parse_bool(value)
            .map(serde_json::Value::Bool)
            .unwrap_or_else(|| json!(value));
    }
    if let Some(value) = evidence_field(&evidence.body, "write_outcome") {
        metadata["write_outcome"] = json!(value);
    }
}

fn evidence_field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
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
    use crate::harness::harness_loop::{
        evidence::timeline::VerifiedActionTimelineStats, state::types::Evidence,
    };

    use super::{add_write_outcome_metadata, verified_action_timeline_metadata};

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

    #[test]
    fn evidence_log_metadata_includes_write_outcome_fields() {
        let evidence = Evidence {
            label: "write:demo.txt".to_string(),
            bytes: 128,
            truncated: false,
            body: "VERIFIED_WRITE_EXECUTION\nexisted_before: true\ncontent_changed: false\nwrite_outcome: unchanged\n".to_string(),
        };
        let mut metadata = serde_json::json!({});

        add_write_outcome_metadata(&mut metadata, &evidence);

        assert_eq!(metadata["existed_before"], true);
        assert_eq!(metadata["content_changed"], false);
        assert_eq!(metadata["write_outcome"], "unchanged");
    }
}
