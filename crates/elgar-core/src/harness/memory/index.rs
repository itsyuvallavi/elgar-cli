//! Builds compact durable memory indexes from session events.

use crate::logs::sessions::LocalSessionLogEvent;

use super::types::{metadata_string, HarnessMemoryFact, HarnessMemoryIndex, HarnessMemoryKind};

pub fn build_memory_index(events: &[LocalSessionLogEvent]) -> HarnessMemoryIndex {
    let mut index = HarnessMemoryIndex::default();

    for event in events {
        match event.kind.as_str() {
            "harness_tool_result_verified" => index_tool_result(&mut index, event),
            "harness_permission_decision" | "harness_approval_decision" => push_metadata_fact(
                &mut index,
                event,
                HarnessMemoryKind::PermissionDecision,
                "tool",
            ),
            "harness_bash_execution_finished"
            | "harness_write_execution_finished"
            | "harness_edit_execution_finished" => index_approved_execution(&mut index, event),
            "harness_turn_finished" | "harness_loop_finished" => push_metadata_fact(
                &mut index,
                event,
                HarnessMemoryKind::StopReason,
                "stop_reason",
            ),
            _ => {}
        }
    }

    index
}

fn index_tool_result(index: &mut HarnessMemoryIndex, event: &LocalSessionLogEvent) {
    if let Some(tool) = metadata_string(&event.metadata, "tool") {
        match tool.as_str() {
            "read" => push_metadata_fact(index, event, HarnessMemoryKind::ReadFile, "path"),
            "ls" => push_metadata_fact(index, event, HarnessMemoryKind::ListedDirectory, "path"),
            "find" => push_compound_fact(
                index,
                event,
                HarnessMemoryKind::FindQuery,
                &["path", "pattern"],
            ),
            "grep" => push_compound_fact(
                index,
                event,
                HarnessMemoryKind::GrepQuery,
                &["path", "query"],
            ),
            _ => {}
        }
        return;
    }

    if let Some(label) = metadata_string(&event.metadata, "evidence_label") {
        index_tool_result_from_evidence_label(index, event, &label);
    }
}

fn index_tool_result_from_evidence_label(
    index: &mut HarnessMemoryIndex,
    event: &LocalSessionLogEvent,
    label: &str,
) {
    let Some((tool, rest)) = label.split_once(':') else {
        return;
    };

    match tool {
        "read" => push_fact(index, event, HarnessMemoryKind::ReadFile, rest.to_string()),
        "ls" => push_fact(
            index,
            event,
            HarnessMemoryKind::ListedDirectory,
            rest.to_string(),
        ),
        "find" => {
            let Some((path, pattern)) = rest.split_once(':') else {
                return;
            };
            push_fact(
                index,
                event,
                HarnessMemoryKind::FindQuery,
                format!("{path}:{pattern}"),
            );
        }
        "grep" => {
            let Some((path, query)) = rest.split_once(':') else {
                return;
            };
            push_fact(
                index,
                event,
                HarnessMemoryKind::GrepQuery,
                format!("{path}:{query}"),
            );
        }
        _ => {}
    }
}

fn index_approved_execution(index: &mut HarnessMemoryIndex, event: &LocalSessionLogEvent) {
    let tool = if let Some(tool) = metadata_string(&event.metadata, "tool") {
        tool
    } else {
        match event.kind.as_str() {
            "harness_bash_execution_finished" => "bash".to_string(),
            "harness_write_execution_finished" => "write".to_string(),
            "harness_edit_execution_finished" => "edit".to_string(),
            _ => return,
        }
    };

    let path = metadata_string(&event.metadata, "path")
        .or_else(|| metadata_string(&event.metadata, "target_requested_path"));
    let key = match path {
        Some(path) => format!("{tool}:{path}"),
        None => tool,
    };
    push_fact(index, event, HarnessMemoryKind::ApprovedExecution, key);
}

fn push_metadata_fact(
    index: &mut HarnessMemoryIndex,
    event: &LocalSessionLogEvent,
    kind: HarnessMemoryKind,
    key_name: &str,
) {
    if let Some(key) = metadata_string(&event.metadata, key_name) {
        push_fact(index, event, kind, key);
    }
}

fn push_compound_fact(
    index: &mut HarnessMemoryIndex,
    event: &LocalSessionLogEvent,
    kind: HarnessMemoryKind,
    key_names: &[&str],
) {
    let parts = key_names
        .iter()
        .filter_map(|key| metadata_string(&event.metadata, key))
        .collect::<Vec<_>>();
    if parts.len() == key_names.len() {
        push_fact(index, event, kind, parts.join(":"));
    }
}

fn push_fact(
    index: &mut HarnessMemoryIndex,
    event: &LocalSessionLogEvent,
    kind: HarnessMemoryKind,
    key: String,
) {
    index.push_unique(HarnessMemoryFact {
        kind,
        key,
        turn_index: event.turn_index,
        source_event: event.kind.clone(),
    });
}
