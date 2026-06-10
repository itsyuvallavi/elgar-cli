//! Same-turn memory system-log events for the primitive harness loop.

use serde_json::json;

use crate::{
    harness::harness_loop::state::memory::HarnessWorkingMemory,
    logs::system::{append_log_event, LogInput, LogPhase},
    session::Session,
};

pub(in crate::harness::harness_loop) fn log_harness_duplicate_rejected(
    session: &Session,
    round_index: usize,
    label: &str,
    memory: &HarnessWorkingMemory,
) {
    let metadata = json!({
        "round_index": round_index,
        "duplicate_label": label,
        "duplicate_requests": memory.duplicate_requests()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_duplicate_rejected",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_duplicate_rejected", metadata);
}

pub(in crate::harness::harness_loop) fn log_harness_memory_snapshot(
    session: &Session,
    round_index: usize,
    reason: &str,
    memory: &HarnessWorkingMemory,
) {
    let metadata = json!({
        "round_index": round_index,
        "reason": reason,
        "listed_paths": memory.listed_paths(),
        "directory_listings": memory.directory_listings().into_iter().map(|listing| {
            json!({
                "path": &listing.path,
                "dirs": &listing.dirs,
                "files": &listing.files,
                "omitted_dirs": listing.omitted_dirs,
                "omitted_files": listing.omitted_files,
                "truncated": listing.truncated
            })
        }).collect::<Vec<_>>(),
        "read_paths": memory.read_paths(),
        "find_patterns": memory.find_patterns(),
        "grep_queries": memory.grep_queries(),
        "duplicate_requests": memory.duplicate_requests()
    });
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            session.next_turn_id(),
            LogPhase::Runtime,
            file!(),
            "run_primitive_harness_loop",
            "harness_memory_snapshot",
        )
        .with_metadata(metadata.clone()),
    );
    session.log_harness_event("harness_memory_snapshot", metadata);
}
