//! Local context file discovery and loading.
//!
//! This file finds candidate files only. It does not decide final prompt shape;
//! `bundle.rs` decides what fits in the budget.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::{
    accounting::{LoadedContextFile, OmittedContextFile},
    budget::estimate_tokens_from_bytes,
};

pub const DEFAULT_CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "elgar-provider.json"];
pub const LOCAL_MEMORY_DIR: &str = ".elgar/memory";
pub const LOCAL_MEMORY_FILE_LIMIT: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContextCandidate {
    pub(super) display_path: String,
    pub(super) bytes: u64,
    pub(super) estimated_tokens: u64,
    pub(super) content: String,
}

impl ContextCandidate {
    /// Convert a selected candidate into the public loaded-file accounting row.
    pub(super) fn loaded_file(&self, truncated: bool, bytes: u64) -> LoadedContextFile {
        LoadedContextFile {
            display_path: self.display_path.clone(),
            bytes,
            estimated_tokens: estimate_tokens_from_bytes(bytes),
            truncated,
        }
    }

    /// Convert a skipped candidate into the public omitted-file accounting row.
    pub(super) fn omitted_file(&self, reason: impl Into<String>) -> OmittedContextFile {
        OmittedContextFile {
            display_path: self.display_path.clone(),
            bytes: self.bytes,
            estimated_tokens: self.estimated_tokens,
            reason: reason.into(),
        }
    }
}

/// Load named context files from project root or cwd.
pub(super) fn load_named_context_candidates<const N: usize>(
    project_root: &Path,
    cwd: &Path,
    file_names: [&str; N],
) -> Vec<ContextCandidate> {
    file_names
        .into_iter()
        .filter_map(|file_name| load_context_candidate(project_root, cwd, file_name))
        .collect()
}

/// Load one context candidate if the file exists and is readable.
fn load_context_candidate(
    project_root: &Path,
    cwd: &Path,
    file_name: &str,
) -> Option<ContextCandidate> {
    let path = existing_file(project_root, cwd, file_name)?;
    let content = fs::read_to_string(&path).ok()?;
    let bytes = content.len() as u64;

    Some(ContextCandidate {
        display_path: file_name.to_string(),
        bytes,
        estimated_tokens: estimate_tokens_from_bytes(bytes),
        content,
    })
}

/// Load markdown memory files from `.elgar/memory`.
///
/// This is intentionally small and bounded so local memory cannot flood the
/// prompt.
pub(super) fn load_local_memory_candidates(project_root: &Path) -> Vec<ContextCandidate> {
    let memory_dir = project_root.join(LOCAL_MEMORY_DIR);
    if !is_real_directory(&memory_dir) {
        return Vec::new();
    }

    let Ok(entries) = fs::read_dir(memory_dir) else {
        return Vec::new();
    };

    let mut files = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "md")
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|entry| entry.file_name());

    files
        .into_iter()
        .take(LOCAL_MEMORY_FILE_LIMIT)
        .filter_map(|entry| {
            let path = entry.path();
            let content = fs::read_to_string(&path).ok()?;
            let bytes = content.len() as u64;
            Some(ContextCandidate {
                display_path: format!("{LOCAL_MEMORY_DIR}/{}", entry.file_name().to_string_lossy()),
                bytes,
                estimated_tokens: estimate_tokens_from_bytes(bytes),
                content,
            })
        })
        .collect()
}

/// Find a named file in project root first, then cwd.
fn existing_file(project_root: &Path, cwd: &Path, file_name: &str) -> Option<PathBuf> {
    [project_root.join(file_name), cwd.join(file_name)]
        .into_iter()
        .find(|path| path.is_file())
}

/// Return true only for real directories, not symlink targets.
fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}
