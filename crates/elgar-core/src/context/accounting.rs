//! Public accounting records for local context loading.
//!
//! These types let the UI explain which files were attached to context, which
//! were skipped, and roughly how many tokens were estimated.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{budget::context_budget_tokens, bundle::ContextBundle};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAccounting {
    pub loaded_files: Vec<LoadedContextFile>,
    #[serde(default)]
    pub omitted_files: Vec<OmittedContextFile>,
    pub estimated_tokens: Option<u64>,
    pub max_window_tokens: Option<u64>,
}

impl ContextAccounting {
    /// Return an empty/unknown accounting value.
    ///
    /// This is used when no context estimation has happened yet.
    pub fn unknown() -> Self {
        Self {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: None,
        }
    }

    /// Return only the accounting for the default local context package.
    pub fn from_default_local_files(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        max_window_tokens: Option<u64>,
    ) -> Self {
        ContextBundle::from_default_local_files(project_root, cwd, max_window_tokens).accounting
    }

    /// Return only the accounting for a caller-provided file list.
    pub fn from_local_files<const N: usize>(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        file_names: [&str; N],
        max_window_tokens: Option<u64>,
    ) -> Self {
        ContextBundle::from_local_files_with_budget(
            project_root,
            cwd,
            file_names,
            max_window_tokens,
            context_budget_tokens(max_window_tokens),
        )
        .accounting
    }
}

impl Default for ContextAccounting {
    fn default() -> Self {
        Self::unknown()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedContextFile {
    pub display_path: String,
    pub bytes: u64,
    pub estimated_tokens: u64,
    #[serde(default)]
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OmittedContextFile {
    pub display_path: String,
    pub bytes: u64,
    pub estimated_tokens: u64,
    pub reason: String,
}
