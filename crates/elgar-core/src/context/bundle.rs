//! Context bundle construction and prompt rendering.
//!
//! A `ContextBundle` contains selected local context sections plus accounting
//! that describes what was loaded or omitted.

use std::path::Path;

use super::{
    accounting::ContextAccounting,
    budget::{
        context_budget_tokens, estimate_tokens_from_bytes, truncate_to_estimated_tokens,
        MIN_TRIMMED_CONTEXT_TOKENS,
    },
    loading::{
        load_local_memory_candidates, load_named_context_candidates, ContextCandidate,
        DEFAULT_CONTEXT_FILES,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextBundle {
    pub accounting: ContextAccounting,
    sections: Vec<ContextSection>,
}

impl ContextBundle {
    /// Load Elgar's default local context files and local memory snippets.
    ///
    /// This is the high-level constructor used when the runtime wants the
    /// normal local context package.
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

    /// Load default context using an explicit budget.
    ///
    /// Tests and future tuning can use this to verify trimming behavior without
    /// changing the global default budget.
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

    /// Load a caller-provided list of local files with an explicit budget.
    ///
    /// This is useful for targeted context selection where the caller already
    /// knows which filenames should be considered.
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

    /// Convert candidate files into selected sections and accounting.
    ///
    /// Files that fit are loaded fully. Files that partially fit are trimmed.
    /// Files that do not fit are recorded as omitted.
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

    /// Build a prompt with context followed by the user's input.
    pub fn prompt_for(&self, input: &str) -> String {
        self.prompt_for_with_recent_conversation(None, input)
    }

    /// Return only the context block, without a user request.
    ///
    /// This is useful for provider APIs that support a separate system/context
    /// message instead of one combined prompt string.
    pub fn system_context(&self) -> Option<String> {
        if self.sections.is_empty() {
            return None;
        }

        let sections = self
            .sections
            .iter()
            .map(ContextSection::render)
            .collect::<Vec<_>>()
            .join("\n\n");
        Some(format!(
            "Local context selected by Elgar runtime:\n{sections}"
        ))
    }

    /// Build a prompt with optional recent conversation and the user's input.
    pub fn prompt_for_with_recent_conversation(
        &self,
        recent_conversation: Option<&str>,
        input: &str,
    ) -> String {
        self.prompt_for_with_recent_conversation_and_verified_memory(
            recent_conversation,
            None,
            input,
        )
    }

    /// Build the full prompt shape used by older context-aware paths.
    ///
    /// Raw chat currently avoids this. When we add full chat, this is one of the
    /// places we should review carefully before attaching context by default.
    pub fn prompt_for_with_recent_conversation_and_verified_memory(
        &self,
        recent_conversation: Option<&str>,
        verified_memory: Option<&str>,
        input: &str,
    ) -> String {
        let recent_conversation = recent_conversation
            .map(str::trim)
            .filter(|conversation| !conversation.is_empty());
        let verified_memory = verified_memory
            .map(str::trim)
            .filter(|memory| !memory.is_empty());

        if self.sections.is_empty() && recent_conversation.is_none() && verified_memory.is_none() {
            return input.to_string();
        }

        let mut blocks = Vec::new();
        if !self.sections.is_empty() {
            let sections = self
                .sections
                .iter()
                .map(ContextSection::render)
                .collect::<Vec<_>>()
                .join("\n\n");
            blocks.push(format!(
                "Local context selected by Elgar controller:\n{sections}"
            ));
        }

        if let Some(recent_conversation) = recent_conversation {
            blocks.push(format!(
                "Recent conversation selected by Elgar controller:\n{recent_conversation}"
            ));
        }

        if let Some(verified_memory) = verified_memory {
            blocks.push(format!(
                "Verified memory selected by Elgar controller:\n{verified_memory}"
            ));
        }

        blocks.push(format!("User request:\n{}", input.trim()));
        blocks.join("\n\n")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextSection {
    display_path: String,
    content: String,
    truncated: bool,
}

impl ContextSection {
    /// Render one context section using a visible file label.
    fn render(&self) -> String {
        let label = if self.truncated {
            format!("{} (truncated)", self.display_path)
        } else {
            self.display_path.clone()
        };
        format!("--- {label} ---\n{}", self.content.trim())
    }
}
