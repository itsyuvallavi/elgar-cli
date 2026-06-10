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
    let Some(tool) = metadata_string(&event.metadata, "tool") else {
        return;
    };
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
}

fn index_approved_execution(index: &mut HarnessMemoryIndex, event: &LocalSessionLogEvent) {
    if let Some(tool) = metadata_string(&event.metadata, "tool") {
        push_fact(index, event, HarnessMemoryKind::ApprovedExecution, tool);
        return;
    }

    let key = match event.kind.as_str() {
        "harness_bash_execution_finished" => "bash",
        "harness_write_execution_finished" => "write",
        "harness_edit_execution_finished" => "edit",
        _ => return,
    };
    push_fact(
        index,
        event,
        HarnessMemoryKind::ApprovedExecution,
        key.to_string(),
    );
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
