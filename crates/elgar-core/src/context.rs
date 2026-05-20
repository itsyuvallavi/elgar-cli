use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "elgar-provider.json"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAccounting {
    pub loaded_files: Vec<LoadedContextFile>,
    pub estimated_tokens: Option<u64>,
    pub max_window_tokens: Option<u64>,
}

impl ContextAccounting {
    pub fn unknown() -> Self {
        Self {
            loaded_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: None,
        }
    }

    pub fn from_default_local_files(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        max_window_tokens: Option<u64>,
    ) -> Self {
        Self::from_local_files(project_root, cwd, DEFAULT_CONTEXT_FILES, max_window_tokens)
    }

    pub fn from_local_files<const N: usize>(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        file_names: [&str; N],
        max_window_tokens: Option<u64>,
    ) -> Self {
        let project_root = project_root.as_ref();
        let cwd = cwd.as_ref();
        let loaded_files = file_names
            .into_iter()
            .filter_map(|file_name| load_context_file(project_root, cwd, file_name))
            .collect::<Vec<_>>();
        let estimated_tokens = if loaded_files.is_empty() {
            None
        } else {
            Some(loaded_files.iter().map(|file| file.estimated_tokens).sum())
        };

        Self {
            loaded_files,
            estimated_tokens,
            max_window_tokens,
        }
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
}

fn load_context_file(
    project_root: &Path,
    cwd: &Path,
    file_name: &str,
) -> Option<LoadedContextFile> {
    let path = existing_file(project_root, cwd, file_name)?;
    let bytes = fs::metadata(&path).ok()?.len();

    Some(LoadedContextFile {
        display_path: file_name.to_string(),
        bytes,
        estimated_tokens: estimate_tokens_from_bytes(bytes),
    })
}

fn existing_file(project_root: &Path, cwd: &Path, file_name: &str) -> Option<PathBuf> {
    [project_root.join(file_name), cwd.join(file_name)]
        .into_iter()
        .find(|path| path.is_file())
}

fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    bytes.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::ContextAccounting;

    #[test]
    fn unknown_context_has_no_fake_counts_or_window() {
        let context = ContextAccounting::unknown();

        assert!(context.loaded_files.is_empty());
        assert_eq!(context.estimated_tokens, None);
        assert_eq!(context.max_window_tokens, None);
    }

    #[test]
    fn default_local_context_tracks_real_files_and_estimated_tokens() {
        let root = temp_root("context-accounting");
        fs::write(root.join("AGENTS.md"), "12345678").unwrap();

        let context = ContextAccounting::from_default_local_files(&root, &root, Some(128_000));

        assert_eq!(context.loaded_files.len(), 1);
        assert_eq!(context.loaded_files[0].display_path, "AGENTS.md");
        assert_eq!(context.loaded_files[0].bytes, 8);
        assert_eq!(context.loaded_files[0].estimated_tokens, 2);
        assert_eq!(context.estimated_tokens, Some(2));
        assert_eq!(context.max_window_tokens, Some(128_000));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_local_context_keeps_usage_unknown() {
        let root = temp_root("context-accounting-empty");

        let context = ContextAccounting::from_default_local_files(&root, &root, Some(128_000));

        assert!(context.loaded_files.is_empty());
        assert_eq!(context.estimated_tokens, None);
        assert_eq!(context.max_window_tokens, Some(128_000));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("elgar-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
