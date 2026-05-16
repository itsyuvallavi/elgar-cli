use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use elgar_cli::render_cli_turn;

fn smoke_root(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-cli-smoke-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

fn render(input: &str, root: &Path) -> String {
    render_cli_turn(input, root, root)
}

#[test]
fn cli_renders_core_events_from_controller_output() {
    let root = smoke_root("core-events");

    let rendered = render("what does the harness do?", &root);

    assert!(rendered.contains("user: what does the harness do?"));
    assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
    assert!(rendered.contains("provider finished: stub-provider request stub-request-1"));
    assert!(rendered.contains("assistant Provider: stub provider response"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn cli_reports_controller_proposal_without_mutating_files() {
    let root = smoke_root("proposal-no-write");
    let target = root.join("hello.py");

    let rendered = render("create file hello.py", &root);

    assert!(rendered.contains("user: create file hello.py"));
    assert!(rendered.contains("action proposed: action-1 WriteFile write hello.py"));
    assert!(rendered.contains(
        "assistant Controller: Proposed WriteFile action. Approve or reject before any file is written."
    ));
    assert!(!target.exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn provider_smoke_command_requires_model_env_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
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
fn controller_smoke_command_requires_model_env_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("controller-smoke")
        .arg("Say hello.")
        .env_remove("ELGAR_LM_STUDIO_MODEL")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ELGAR_LM_STUDIO_MODEL"));
    assert!(output.stdout.is_empty());
}

#[test]
fn tui_controller_smoke_command_requires_model_env_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui-controller-smoke")
        .arg("Say hello.")
        .env_remove("ELGAR_LM_STUDIO_MODEL")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ELGAR_LM_STUDIO_MODEL"));
    assert!(output.stdout.is_empty());
}

#[test]
fn tui_controller_smoke_command_renders_tui_provider_error_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui-controller-smoke")
        .arg("Say hello.")
        .env("ELGAR_LM_STUDIO_MODEL", "local-model")
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Conversation\n"));
    assert!(stdout.contains("You: Say hello."));
    assert!(
        stdout.contains("Provider progress: working with lm-studio (request lm-studio-request-1).")
    );
    assert!(stdout.contains(
        "Provider error from lm-studio: Configuration provider error: only http:// provider URLs are supported"
    ));
    assert!(stdout.contains("Status\nprovider error"));
    assert!(!stdout.contains("stub-provider"));
}

#[test]
fn tui_command_reads_stdin_renders_stub_turn_and_exits() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui")
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

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
    assert!(stdout.contains("Elgar TUI. Type /exit or /quit to leave."));
    assert!(stdout.contains("You: what does the harness do?"));
    assert!(
        stdout.contains("Provider progress: working with stub-provider (request stub-request-1).")
    );
    assert!(stdout.contains("Assistant suggestion: stub provider response"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("lm-studio"));
}

#[test]
fn tui_command_help_is_local_and_does_not_call_provider() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui")
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

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
    assert!(stdout.contains("Elgar TUI commands:"));
    assert!(stdout.contains("/approve"));
    assert!(stdout.contains("/reject"));
    assert!(stdout.contains("/copy"));
    assert!(stdout.contains("/help"));
    assert!(stdout.contains("/commands"));
    assert!(stdout.contains("/exit"));
    assert!(stdout.contains("/quit"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("/model"));
    assert!(!stdout.contains("/settings"));
    assert!(!stdout.contains("You: /help"));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("stub-provider"));
    assert!(!stdout.contains("lm-studio"));
}

#[test]
fn tui_command_rejects_pending_action_with_slash_command_without_network() {
    let root = smoke_root("slash-reject");
    let target = root.join("rejected.py");

    let mut child = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui")
        .current_dir(&root)
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"create file rejected.py\n/reject\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(!target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("State: rejected"));
    assert!(stdout.contains("Result: Rejected. No file was changed."));
    assert!(stdout.contains("Rejected actions are final"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_approves_pending_action_with_slash_command_without_network() {
    let root = smoke_root("slash-approve");
    let target = root.join("approved.py");

    let mut child = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui")
        .current_dir(&root)
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"create file approved.py\n/approve\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("State: applied and verified"));
    assert!(stdout.contains("Result: file written:"));
    assert!(stdout.contains("approved.py"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_line_loop_preserves_controller_backed_action_lifecycle() {
    let root = smoke_root("line-loop-lifecycle");
    let rejected_target = root.join("rejected.py");
    let approved_target = root.join("approved.py");

    let mut child = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("tui")
        .current_dir(&root)
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(
            b"create file rejected.py\n/reject\n/approve\ncreate file approved.py\n/approve\n/exit\n",
        )
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(!rejected_target.exists());
    assert!(approved_target.exists());
    assert_eq!(fs::read_to_string(&approved_target).unwrap(), "");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("You: create file rejected.py"));
    assert!(stdout.contains("Action: action-1 WriteFile"));
    assert!(stdout.contains("Target: rejected.py"));
    assert!(stdout.contains("State: waiting for approval"));
    assert!(stdout.contains("State: rejected"));
    assert!(stdout.contains("Result: Rejected. No file was changed."));
    assert!(stdout.contains("Rejected actions are final"));
    assert!(stdout.contains("No proposed action is waiting for approval."));
    assert!(stdout.contains("You: create file approved.py"));
    assert!(stdout.contains("Action: action-2 WriteFile"));
    assert!(stdout.contains("Target: approved.py"));
    assert!(stdout.contains("State: applied and verified"));
    assert!(stdout.contains("Result: file written:"));
    assert!(stdout.contains("approved.py"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));
    assert!(!stdout.contains("LM Studio smoke failed"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn default_cli_path_uses_stub_controller_even_when_lm_studio_env_is_set() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .arg("what does the harness do?")
        .env(
            "ELGAR_LM_STUDIO_MODEL",
            "loaded-model-that-must-not-be-used",
        )
        .env("ELGAR_LM_STUDIO_BASE_URL", "https://127.0.0.1:1234/v1")
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
