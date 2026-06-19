//! Path matching for direct primitive target fidelity.

use crate::harness::harness_loop::evidence::keys::normalize_evidence_path;

pub(super) fn direct_path_matches(expected_path: &str, actual_path: &str) -> bool {
    let expected = normalize_evidence_path(expected_path);
    let actual = normalize_evidence_path(actual_path);
    expected == actual || basename_context_match(&expected, &actual)
}

fn basename_context_match(expected_path: &str, actual_path: &str) -> bool {
    if expected_path.contains('/') || actual_path_has_ignored_segment(actual_path) {
        return false;
    }
    actual_path
        .rsplit('/')
        .next()
        .is_some_and(|name| name == expected_path)
}

fn actual_path_has_ignored_segment(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(
            segment,
            ".git" | ".next" | "node_modules" | "target" | "dist" | "build"
        )
    })
}
