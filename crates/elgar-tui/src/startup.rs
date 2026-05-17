use std::path::Path;

pub const CONTEXT_FILES: [&str; 2] = ["AGENTS.md", "elgar-provider.json"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBlock {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub context_files: Vec<String>,
}

impl StartupBlock {
    pub fn new(
        project_root: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        provider: Option<String>,
        model: Option<String>,
    ) -> Self {
        Self {
            provider,
            model,
            context_files: available_context_files(project_root.as_ref(), cwd.as_ref()),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "elgar v0.2\n/commands · /clear · /approve · /reject · /copy · /exit\n\nElgar uses your local LM Studio model and keeps file changes behind approval.\n\n[Context]\n{}\n\n[Provider]\n  {} · {}",
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

fn available_context_files(project_root: &Path, cwd: &Path) -> Vec<String> {
    CONTEXT_FILES
        .iter()
        .filter(|file_name| file_exists(project_root, file_name) || file_exists(cwd, file_name))
        .map(|file_name| (*file_name).to_string())
        .collect()
}

fn file_exists(root: &Path, file_name: &str) -> bool {
    root.join(file_name).is_file()
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
            "elgar v0.2\n/commands · /clear · /approve · /reject · /copy · /exit\n\nElgar uses your local LM Studio model and keeps file changes behind approval.\n\n[Context]\n  AGENTS.md\n\n[Provider]\n  lm-studio · openai/gpt-oss-20b"
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
