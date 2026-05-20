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
    #[cfg(test)]
    pub fn new(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        provider: Option<String>,
        model: Option<String>,
    ) -> Self {
        let context = ContextAccounting::from_default_local_files(project_root, cwd, None);
        Self::from_context_accounting(provider, model, &context)
    }

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
            "elgar v0.2\n/commands · /clear · /cancel · /approve · /reject · /copy · /exit\n\nElgar uses your local LM Studio model and keeps file changes behind approval.\n\n[Context]\n{}\n\n[Provider]\n  {} · {}",
            self.render_context_files(),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none")
        )
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
mod tests {
    use std::fs;

    use super::StartupBlock;

    #[test]
    fn startup_block_lists_only_real_context_files_and_provider() {
        let root = temp_root("startup-context");
        fs::write(root.join("AGENTS.md"), "agent instructions").unwrap();

        let block = StartupBlock::new(
            &root,
            &root,
            Some("lm-studio".to_string()),
            Some("openai/gpt-oss-20b".to_string()),
        );

        let rendered = block.render();

        assert_eq!(
            rendered,
            "elgar v0.2\n/commands · /clear · /cancel · /approve · /reject · /copy · /exit\n\nElgar uses your local LM Studio model and keeps file changes behind approval.\n\n[Context]\n  AGENTS.md\n\n[Provider]\n  lm-studio · openai/gpt-oss-20b"
        );
        assert!(!rendered.contains("elgar-provider.json"));
        assert!(!rendered.contains("Commands:"));
        assert!(!rendered.contains("Skills"));
        assert!(!rendered.contains("MCP"));
        assert!(!rendered.contains("Bash"));
        assert!(!rendered.contains("API"));
        assert!(!rendered.contains("settings"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_block_uses_none_for_missing_context_provider_and_model() {
        let root = temp_root("startup-empty");

        let rendered = StartupBlock::new(&root, &root, None, None).render();

        assert!(rendered.contains("[Context]\n  (none)"));
        assert!(rendered.contains("[Provider]\n  none · none"));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-startup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
