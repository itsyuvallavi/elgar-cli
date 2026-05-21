use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const DEFAULT_CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "elgar-provider.json"];
pub const LOCAL_MEMORY_DIR: &str = ".elgar/memory";
pub const LOCAL_MEMORY_FILE_LIMIT: usize = 8;
pub const DEFAULT_CONTEXT_BUDGET_TOKENS: u64 = 768;
const MIN_TRIMMED_CONTEXT_TOKENS: u64 = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub accounting: ContextAccounting,
    sections: Vec<ContextSection>,
}

impl ContextBundle {
    pub fn from_default_local_files(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        max_window_tokens: Option<u64>,
    ) -> Self {
        Self::from_default_local_files_with_budget(
            project_root,
            cwd,
            max_window_tokens,
            context_budget_tokens(max_window_tokens),
        )
    }

    pub fn from_default_local_files_with_budget(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        max_window_tokens: Option<u64>,
        budget_tokens: u64,
    ) -> Self {
        let project_root = project_root.as_ref();
        let cwd = cwd.as_ref();
        let mut candidates =
            load_named_context_candidates(project_root, cwd, DEFAULT_CONTEXT_FILES);
        candidates.extend(load_local_memory_candidates(project_root));
        Self::from_candidates_with_budget(candidates, max_window_tokens, budget_tokens)
    }

    pub fn from_local_files_with_budget<const N: usize>(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        file_names: [&str; N],
        max_window_tokens: Option<u64>,
        budget_tokens: u64,
    ) -> Self {
        let project_root = project_root.as_ref();
        let cwd = cwd.as_ref();
        let candidates = load_named_context_candidates(project_root, cwd, file_names);
        Self::from_candidates_with_budget(candidates, max_window_tokens, budget_tokens)
    }

    fn from_candidates_with_budget(
        candidates: Vec<ContextCandidate>,
        max_window_tokens: Option<u64>,
        budget_tokens: u64,
    ) -> Self {
        let mut used_tokens = 0;
        let mut loaded_files = Vec::new();
        let mut omitted_files = Vec::new();
        let mut sections = Vec::new();

        for candidate in candidates {
            let remaining_tokens = budget_tokens.saturating_sub(used_tokens);

            if candidate.estimated_tokens <= remaining_tokens {
                used_tokens += candidate.estimated_tokens;
                loaded_files.push(candidate.loaded_file(false, candidate.bytes));
                sections.push(ContextSection {
                    display_path: candidate.display_path,
                    content: candidate.content,
                    truncated: false,
                });
            } else if remaining_tokens >= MIN_TRIMMED_CONTEXT_TOKENS {
                let trimmed = truncate_to_estimated_tokens(&candidate.content, remaining_tokens);
                let trimmed_bytes = trimmed.len() as u64;
                used_tokens += estimate_tokens_from_bytes(trimmed_bytes);
                loaded_files.push(candidate.loaded_file(true, trimmed_bytes));
                sections.push(ContextSection {
                    display_path: candidate.display_path,
                    content: trimmed,
                    truncated: true,
                });
            } else {
                omitted_files.push(candidate.omitted_file("context budget exceeded"));
            }
        }

        let estimated_tokens = if loaded_files.is_empty() {
            None
        } else {
            Some(used_tokens)
        };

        Self {
            accounting: ContextAccounting {
                loaded_files,
                omitted_files,
                estimated_tokens,
                max_window_tokens,
            },
            sections,
        }
    }

    pub fn prompt_for(&self, input: &str) -> String {
        if self.sections.is_empty() {
            return input.to_string();
        }

        let sections = self
            .sections
            .iter()
            .map(ContextSection::render)
            .collect::<Vec<_>>()
            .join("\n\n");
        format!(
            "Local context selected by Elgar controller:\n{sections}\n\nUser request:\n{}",
            input.trim()
        )
    }
}

fn load_named_context_candidates<const N: usize>(
    project_root: &Path,
    cwd: &Path,
    file_names: [&str; N],
) -> Vec<ContextCandidate> {
    file_names
        .into_iter()
        .filter_map(|file_name| load_context_candidate(project_root, cwd, file_name))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextSection {
    display_path: String,
    content: String,
    truncated: bool,
}

impl ContextSection {
    fn render(&self) -> String {
        let label = if self.truncated {
            format!("{} (truncated)", self.display_path)
        } else {
            self.display_path.clone()
        };
        format!("--- {label} ---\n{}", self.content.trim())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAccounting {
    pub loaded_files: Vec<LoadedContextFile>,
    #[serde(default)]
    pub omitted_files: Vec<OmittedContextFile>,
    pub estimated_tokens: Option<u64>,
    pub max_window_tokens: Option<u64>,
}

impl ContextAccounting {
    pub fn unknown() -> Self {
        Self {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: None,
        }
    }

    pub fn from_default_local_files(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        max_window_tokens: Option<u64>,
    ) -> Self {
        ContextBundle::from_default_local_files(project_root, cwd, max_window_tokens).accounting
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextCandidate {
    display_path: String,
    bytes: u64,
    estimated_tokens: u64,
    content: String,
}

impl ContextCandidate {
    fn loaded_file(&self, truncated: bool, bytes: u64) -> LoadedContextFile {
        LoadedContextFile {
            display_path: self.display_path.clone(),
            bytes,
            estimated_tokens: estimate_tokens_from_bytes(bytes),
            truncated,
        }
    }

    fn omitted_file(&self, reason: impl Into<String>) -> OmittedContextFile {
        OmittedContextFile {
            display_path: self.display_path.clone(),
            bytes: self.bytes,
            estimated_tokens: self.estimated_tokens,
            reason: reason.into(),
        }
    }
}

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

fn load_local_memory_candidates(project_root: &Path) -> Vec<ContextCandidate> {
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

fn existing_file(project_root: &Path, cwd: &Path, file_name: &str) -> Option<PathBuf> {
    [project_root.join(file_name), cwd.join(file_name)]
        .into_iter()
        .find(|path| path.is_file())
}

fn is_real_directory(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|metadata| metadata.file_type().is_dir())
        .unwrap_or(false)
}

fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    bytes.div_ceil(4)
}

fn context_budget_tokens(max_window_tokens: Option<u64>) -> u64 {
    match max_window_tokens {
        Some(max) => DEFAULT_CONTEXT_BUDGET_TOKENS.min(max.saturating_sub(256)),
        None => DEFAULT_CONTEXT_BUDGET_TOKENS,
    }
}

fn truncate_to_estimated_tokens(content: &str, tokens: u64) -> String {
    let max_bytes = tokens.saturating_mul(4) as usize;
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_string()
}
