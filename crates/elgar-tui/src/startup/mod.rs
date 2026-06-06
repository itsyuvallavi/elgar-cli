//! Startup text and context summary helpers.
//!
//! This file builds the first visible status block when the TUI starts.

#[cfg(test)]
use std::path::Path;

use elgar_core::context::ContextAccounting;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBlock {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_files: Vec<String>,
}

impl StartupBlock {
    /// Build the startup block from local files. Used by tests.
    #[cfg(test)]
    pub(crate) fn new(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        provider: Option<String>,
        model: Option<String>,
    ) -> Self {
        let context = ContextAccounting::from_default_local_files(project_root, cwd, None);
        Self::from_context_accounting(provider, model, &context)
    }

    /// Build the startup block from already-computed context accounting.
    pub fn from_context_accounting(
        provider: Option<String>,
        model: Option<String>,
        context: &ContextAccounting,
    ) -> Self {
        Self {
            provider,
            model,
            context_files: context
                .loaded_files
                .iter()
                .map(|file| file.display_path.clone())
                .collect(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "elgar v0.10\n/commands · /clear · /copy · /exit\n\n{}\n\n[Context]\n{}\n\n[Provider]\n  {} · {}",
            self.provider_description(),
            self.render_context_files(),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none")
        )
    }

    fn provider_description(&self) -> String {
        match self.provider.as_deref() {
            Some("lm-studio") => "Elgar uses your local LM Studio model.".to_string(),
            Some("stub-provider") => {
                "Elgar is running with the default no-network stub provider.".to_string()
            }
            _ => "Elgar is ready.".to_string(),
        }
    }

    fn render_context_files(&self) -> String {
        if self.context_files.is_empty() {
            "  (none)".to_string()
        } else {
            self.context_files
                .iter()
                .map(|file_name| format!("  {file_name}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

#[cfg(test)]
mod tests;
