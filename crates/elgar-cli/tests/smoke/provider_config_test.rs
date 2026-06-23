//! Provider config lookup smoke tests.

use std::{fs, io::Write};

use super::support::{elgar_command, scripted_tui_command, smoke_root, write_live_provider_config};

#[test]
fn provider_smoke_command_requires_model_env_without_network() {
    let output = elgar_command()
        .arg("provider-smoke")
        .arg("Say hello.")
        .env_remove("ELGAR_LM_STUDIO_MODEL")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ELGAR_LM_STUDIO_MODEL"));
    assert!(output.stdout.is_empty());
}

#[test]
fn normal_cli_uses_repo_provider_config_when_present() {
    let root = smoke_root("runtime-config-cli");
    write_live_provider_config(&root);

    let output = elgar_command()
        .current_dir(&root)
        .arg("Say hello in one sentence.")
        .env_remove("ELGAR_PROVIDER_CONFIG")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("user: Say hello in one sentence."));
    assert!(stdout.contains("provider started: lm-studio request lm-studio-request-1"));
    assert!(stdout.contains("only http:// provider URLs are supported"));
    assert!(!stdout.contains("stub-provider"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn normal_cli_uses_project_root_override_when_launched_outside_repo() {
    let root = smoke_root("runtime-config-cli-root-override");
    let outside = smoke_root("runtime-config-cli-outside");
    write_live_provider_config(&root);

    let output = elgar_command()
        .current_dir(&outside)
        .arg("Say hello in one sentence.")
        .env("ELGAR_PROJECT_ROOT", &root)
        .env_remove("ELGAR_PROVIDER_CONFIG")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("user: Say hello in one sentence."));
    assert!(stdout.contains("provider started: lm-studio request lm-studio-request-1"));
    assert!(stdout.contains("only http:// provider URLs are supported"));
    assert!(!stdout.contains("stub-provider"));

    let _ = fs::remove_dir_all(outside);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_line_loop_uses_repo_provider_config_when_present() {
    let root = smoke_root("runtime-config-tui-loop");
    write_live_provider_config(&root);

    let mut child = scripted_tui_command()
        .current_dir(&root)
        .env_remove("ELGAR_PROVIDER_CONFIG")
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"Say hello in one sentence.\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> Say hello in one sentence."));
    assert!(stdout.contains("only http:// provider URLs are supported"));
    assert!(!stdout.contains("stub provider response"));
    assert!(!stdout.contains("No live provider call was made"));
    assert!(stdout.contains("Exiting Elgar TUI."));

    let _ = fs::remove_dir_all(root);
}
