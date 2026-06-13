//! Runtime session id helpers.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static SESSION_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Builds a unique local session id for one launched runtime surface.
pub fn runtime_session_id(prefix: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let counter = SESSION_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}-{}-{millis}-{counter}", std::process::id())
}

pub(super) fn rotate_session_id(current: &str) -> String {
    if let Some((base, suffix)) = current.rsplit_once("-clear-") {
        if let Ok(generation) = suffix.parse::<u32>() {
            return format!("{base}-clear-{}", generation + 1);
        }
    }
    format!("{current}-clear-1")
}
