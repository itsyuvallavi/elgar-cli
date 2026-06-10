//! Shared path formatting helpers for read-only harness collectors.

use std::path::{Path, PathBuf};

pub(super) fn resolve_optional_directory_path(root: &Path, path: &str) -> PathBuf {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "." {
        return root.to_path_buf();
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

pub(super) fn display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
