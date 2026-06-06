//! Runtime path discovery for CLI launches.
//!
//! The CLI can be launched from inside the repo, outside the repo, or with
//! environment overrides. This file resolves the project root and config path.

use std::path::{Path, PathBuf};

pub const PROVIDER_CONFIG_ENV: &str = "ELGAR_PROVIDER_CONFIG";
pub const PROJECT_ROOT_ENV: &str = "ELGAR_PROJECT_ROOT";
pub const PROVIDER_CONFIG_FILE: &str = "elgar-provider.json";
const AGENTS_FILE: &str = "AGENTS.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimePaths {
    pub project_root: PathBuf,
    pub cwd: PathBuf,
}

impl RuntimePaths {
    /// Resolves paths from the process current working directory.
    pub fn from_current_dir() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::from_cwd(cwd)
    }

    /// Resolves paths from a supplied cwd, useful for tests and scripted runs.
    pub fn from_cwd(cwd: impl AsRef<Path>) -> Self {
        let cwd = cwd.as_ref().to_path_buf();
        let project_root = resolve_runtime_project_root(&cwd, installed_project_root());
        Self { project_root, cwd }
    }
}

/// Finds the project root for this runtime invocation.
pub fn resolve_runtime_project_root(
    cwd: impl AsRef<Path>,
    installed_root: Option<PathBuf>,
) -> PathBuf {
    let cwd = cwd.as_ref();

    env_project_root()
        .or_else(explicit_provider_config_root)
        .or_else(|| find_elgar_project_root(cwd))
        .or_else(|| installed_root.filter(|root| is_elgar_project_root(root)))
        .unwrap_or_else(|| cwd.to_path_buf())
}

pub(crate) fn find_provider_config_file(start: impl AsRef<Path>) -> Option<PathBuf> {
    let mut current = start.as_ref();
    loop {
        let candidate = current.join(PROVIDER_CONFIG_FILE);
        if candidate.exists() {
            return Some(candidate);
        }

        let parent = current.parent()?;
        current = parent;
    }
}

fn installed_project_root() -> Option<PathBuf> {
    option_env!("ELGAR_INSTALL_REPO_ROOT")
        .map(str::trim)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
}

fn env_project_root() -> Option<PathBuf> {
    std::env::var(PROJECT_ROOT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|root| is_elgar_project_root(root))
}

fn explicit_provider_config_root() -> Option<PathBuf> {
    let value = std::env::var(PROVIDER_CONFIG_ENV).ok()?;
    let trimmed = value.trim();
    if matches!(trimmed, "" | "off" | "none" | "disabled") {
        return None;
    }

    Path::new(trimmed)
        .parent()
        .map(Path::to_path_buf)
        .filter(|root| is_elgar_project_root(root))
}

fn find_elgar_project_root(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if is_elgar_project_root(current) {
            return Some(current.to_path_buf());
        }

        let parent = current.parent()?;
        current = parent;
    }
}

fn is_elgar_project_root(root: &Path) -> bool {
    root.join(PROVIDER_CONFIG_FILE).exists() || root.join(AGENTS_FILE).exists()
}
