//! Single-turn CLI runtime smoke tests.

use std::fs;

use super::support::{elgar_command, force_stub_env, render, smoke_root};

#[test]
fn cli_renders_core_events_from_agent_runtime_output() {
    let root = smoke_root("core-events");

    let rendered = render("what does the harness do?", &root);

    assert!(rendered.contains("user: what does the harness do?"));
    assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
    assert!(rendered.contains("provider finished: stub-provider request stub-request-1"));
    assert!(rendered.contains("assistant Provider: stub provider response"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn cli_default_agent_runtime_does_not_create_files_from_plain_text() {
    let root = smoke_root("plain-text-no-create-file");
    let target = root.join("hello.py");

    let rendered = render("create file hello.py", &root);

    assert!(rendered.contains("user: create file hello.py"));
    assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
    assert!(rendered.contains("assistant Provider: stub provider response"));
    assert!(!rendered.contains("action approved"));
    assert!(!rendered.contains("action applied"));
    assert!(!target.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn zero_arg_elgar_non_interactive_keeps_placeholder_without_hanging() {
    let output = elgar_command()
        .env_remove("ELGAR_PROVIDER_CONFIG")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Elgar v0.10 is ready"));
}

#[test]
fn default_cli_path_uses_stub_agent_runtime_even_when_lm_studio_env_is_set() {
    let output = force_stub_env(elgar_command().arg("what does the harness do?"))
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("provider started: stub-provider request stub-request-1"));
    assert!(stdout.contains("provider finished: stub-provider request stub-request-1"));
    assert!(stdout.contains("assistant Provider: stub provider response"));
    assert!(!stdout.contains("lm-studio"));
    assert!(!stdout.contains("LM Studio smoke failed"));
}
