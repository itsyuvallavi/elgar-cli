//! Scripted TUI smoke tests for local command behavior.

use std::io::Write;

use super::support::{force_stub_env, scripted_tui_command};

#[test]
fn tui_command_reads_stdin_renders_stub_turn_and_exits() {
    let mut child = force_stub_env(&mut scripted_tui_command()).spawn().unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"what does the harness do?\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Elgar TUI. Type /exit, /quit, or /q to leave."));
    assert!(stdout.contains("> what does the harness do?"));
    assert!(stdout.contains("stub provider response"));
    assert!(!stdout.contains("Model:"));
    assert!(!stdout.contains("stub-request-1"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("lm-studio"));
}

#[test]
fn tui_command_greeting_gets_stub_guidance_without_live_provider() {
    let mut child = force_stub_env(&mut scripted_tui_command()).spawn().unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"hello!\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> hello!"));
    assert!(stdout.contains("stub provider response (no-network) to: hello!"));
    assert!(stdout.contains("No live provider call was made"));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));
}

#[test]
fn tui_command_help_is_local_and_does_not_call_provider() {
    let mut child = force_stub_env(&mut scripted_tui_command()).spawn().unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"/help\n/commands\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Commands\nChat"));
    assert!(stdout.contains("View"));
    assert!(stdout.contains("Exit"));
    assert!(stdout.contains("/clear"));
    assert!(stdout.contains("/new"));
    assert!(stdout.contains("/cancel"));
    assert!(stdout.contains("/details last"));
    assert!(stdout.contains("/copy raw"));
    assert!(stdout.contains("/copy"));
    assert!(stdout.contains("/help"));
    assert!(stdout.contains("/commands"));
    assert!(stdout.contains("/exit"));
    assert!(stdout.contains("/quit"));
    assert!(stdout.contains("/q"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("/model"));
    assert!(!stdout.contains("/settings"));
    assert!(!stdout.contains("/bash"));
    assert!(!stdout.contains("/api"));
    assert!(!stdout.contains("> /help"));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("stub-provider"));
    assert!(!stdout.contains("lm-studio"));
}

#[test]
fn tui_command_prompt_block_submits_one_multiline_turn() {
    let mut child = force_stub_env(&mut scripted_tui_command()).spawn().unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"/prompt\nCreate a project.\nRequirements:\n- include README.md\n/end\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> Create a project."));
    assert!(stdout.contains("Requirements:"));
    assert!(stdout.contains("- include README.md"));
    assert_eq!(stdout.matches("stub provider response").count(), 1);
    assert!(stdout.contains("Exiting Elgar TUI."));
}
