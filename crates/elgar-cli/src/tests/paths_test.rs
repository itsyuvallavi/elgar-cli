//! Tests for CLI project-root path resolution.

use std::{fs, path::PathBuf};

use crate::{resolve_runtime_project_root, RuntimePaths, PROVIDER_CONFIG_FILE};

fn temp_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-lib-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn runtime_project_root_uses_installed_root_when_cwd_has_no_config() {
    let installed = temp_root("runtime-installed-root");
    let outside = temp_root("runtime-outside-root");
    fs::write(installed.join(PROVIDER_CONFIG_FILE), "{}").unwrap();

    let resolved = resolve_runtime_project_root(&outside, Some(installed.clone()));

    assert_eq!(resolved, installed);

    let _ = fs::remove_dir_all(outside);
    let _ = fs::remove_dir_all(installed);
}

#[test]
fn runtime_project_root_prefers_cwd_config_over_installed_root() {
    let installed = temp_root("runtime-installed-root-cwd-loses");
    let workspace = temp_root("runtime-workspace-root");
    let child = workspace.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(installed.join(PROVIDER_CONFIG_FILE), "{}").unwrap();
    fs::write(workspace.join(PROVIDER_CONFIG_FILE), "{}").unwrap();

    let resolved = resolve_runtime_project_root(&child, Some(installed.clone()));

    assert_eq!(resolved, workspace);

    let _ = fs::remove_dir_all(child);
    let _ = fs::remove_dir_all(workspace);
    let _ = fs::remove_dir_all(installed);
}

#[test]
fn runtime_paths_store_project_root_and_cwd() {
    let workspace = temp_root("runtime-paths-cwd");
    let child = workspace.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::write(workspace.join(PROVIDER_CONFIG_FILE), "{}").unwrap();

    let paths = RuntimePaths::from_cwd(&child);

    assert_eq!(paths.project_root, workspace);
    assert_eq!(paths.cwd, child);

    let _ = fs::remove_dir_all(paths.project_root);
}
