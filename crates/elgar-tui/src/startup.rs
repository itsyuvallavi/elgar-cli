#[cfg(test)]
use std::path::Path;

use elgar_core::{context::ContextAccounting, policy::PermissionPolicyMode};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupBlock {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub policy_mode: PermissionPolicyMode,
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
        Self::from_context_accounting(
            provider,
            model,
            PermissionPolicyMode::AutoCreateReviewModify,
            &context,
        )
    }

    pub fn from_context_accounting(
        provider: Option<String>,
        model: Option<String>,
        policy_mode: PermissionPolicyMode,
        context: &ContextAccounting,
    ) -> Self {
        Self {
            provider,
            model,
            policy_mode,
            context_files: context
                .loaded_files
                .iter()
                .map(|file| file.display_path.clone())
                .collect(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "elgar v0.2\n/commands · /permissions · /clear · /approve · /reject · /copy · /exit\n\n{}\n\n[Context]\n{}\n\n[Provider]\n  {} · {}\n\n[Policy]\n  {}",
            self.provider_description(),
            self.render_context_files(),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none"),
            self.policy_mode
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
            "elgar v0.2\n/commands · /permissions · /clear · /approve · /reject · /copy · /exit\n\nElgar uses your local LM Studio model.\n\n[Context]\n  AGENTS.md\n\n[Provider]\n  lm-studio · openai/gpt-oss-20b\n\n[Policy]\n  auto_create_review_modify"
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

        assert!(!rendered.contains("local LM Studio model"));
        assert!(rendered.contains("[Context]\n  (none)"));
        assert!(rendered.contains("[Provider]\n  none · none"));
        assert!(rendered.contains("[Policy]\n  auto_create_review_modify"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_block_does_not_claim_lm_studio_for_stub_provider() {
        let root = temp_root("startup-stub");

        let rendered =
            StartupBlock::new(&root, &root, Some("stub-provider".to_string()), None).render();

        assert!(!rendered.contains("local LM Studio model"));
        assert!(rendered.contains("[Provider]\n  stub-provider · none"));
        assert!(rendered.contains("[Policy]\n  auto_create_review_modify"));

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
