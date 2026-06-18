//! Startup text and context summary helpers.
//!
//! This file builds the first visible status block when the TUI starts.

#[cfg(test)]
use std::path::Path;

use elgar_core::context::ContextAccounting;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMcpStatus {
    Inactive,
    Active {
        server_ids: Vec<String>,
        source_path: String,
    },
    Error {
        message: String,
    },
}

impl StartupMcpStatus {
    pub fn active(server_ids: Vec<String>, source_path: impl Into<String>) -> Self {
        Self::Active {
            server_ids,
            source_path: source_path.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }

    fn render(&self) -> String {
        match self {
            Self::Inactive => "  inactive".to_string(),
            Self::Active {
                server_ids,
                source_path,
            } => {
                let servers = if server_ids.is_empty() {
                    "?".to_string()
                } else {
                    server_ids.join(", ")
                };
                format!("  active · {servers}\n  source · {source_path}")
            }
            Self::Error { message } => format!("  error · {message}"),
        }
    }
}

impl Default for StartupMcpStatus {
    fn default() -> Self {
        Self::Inactive
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBlock {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_files: Vec<String>,
    pub mcp_status: StartupMcpStatus,
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
        Self::from_context_accounting_with_mcp(
            provider,
            model,
            &context,
            StartupMcpStatus::Inactive,
        )
    }

    /// Build the startup block with explicit MCP display status.
    pub fn from_context_accounting_with_mcp(
        provider: Option<String>,
        model: Option<String>,
        context: &ContextAccounting,
        mcp_status: StartupMcpStatus,
    ) -> Self {
        Self {
            provider,
            model,
            context_files: context
                .loaded_files
                .iter()
                .map(|file| file.display_path.clone())
                .collect(),
            mcp_status,
        }
    }

    pub fn render(&self) -> String {
        format!(
            "elgar v0.10\n/commands · /clear · /copy · /exit\n\n{}\n\n[Context]\n{}\n\n[Provider]\n  {} · {}\n\n[MCP]\n{}",
            self.provider_description(),
            self.render_context_files(),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none"),
            self.mcp_status.render()
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
