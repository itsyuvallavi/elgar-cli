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
    assert!(rendered.contains("action proposed: action-1 CreateFile write hello.py"));
    assert!(rendered.contains(
        "assistant Controller: Proposed CreateFile action. Approve or reject before any file is written."
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
fn zero_arg_elgar_non_interactive_keeps_placeholder_without_hanging() {
    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
        .env_remove("ELGAR_PROVIDER_CONFIG")
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Elgar v0.2 is ready"));
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
    assert!(stdout.contains("> Say hello."));
    assert!(!stdout.contains("lm-studio-request-1"));
    assert!(stdout.contains(
        "Provider error from lm-studio: Configuration provider error: only http:// provider URLs are supported"
    ));
    assert!(stdout.contains("Status\nprovider error"));
    assert!(!stdout.contains("stub-provider"));
}

#[test]
fn normal_cli_uses_repo_provider_config_when_present() {
    let root = smoke_root("runtime-config-cli");
    fs::write(
        root.join("elgar-provider.json"),
        r#"{
          "provider": "lm-studio",
          "base_url": "https://127.0.0.1:1234/v1",
          "default_model": "openai/gpt-oss-20b",
          "mode": "live"
        }"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
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
    fs::write(
        root.join("elgar-provider.json"),
        r#"{
          "provider": "lm-studio",
          "base_url": "https://127.0.0.1:1234/v1",
          "default_model": "openai/gpt-oss-20b",
          "mode": "live"
        }"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_elgar"))
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
        .write_all(b"hello!\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> hello!"));
    assert!(stdout.contains("stub provider response (no-network) to: hello!"));
    assert!(stdout.contains("No live provider call was made"));
    assert!(stdout.contains("tui-controller-smoke"));
    assert!(!stdout.contains("Input was not recognized"));
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
    assert!(stdout.contains("Commands\n/commands"));
    assert!(stdout.contains("/clear"));
    assert!(stdout.contains("/new"));
    assert!(stdout.contains("/approve"));
    assert!(stdout.contains("/reject"));
    assert!(stdout.contains("/memory"));
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
    assert!(target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(&format!(
        "Wrote {}.",
        fs::canonicalize(&target).unwrap().display()
    )));
    assert!(stdout.contains("Pending Action\nnone"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("No proposed action is waiting for rejection."));
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
    assert!(stdout.contains(&format!(
        "Wrote {}.",
        fs::canonicalize(&target).unwrap().display()
    )));
    assert!(stdout.contains("Pending Action\nnone"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_applies_shell_action_after_manual_approval() {
    let root = smoke_root("slash-approve-shell");
    let target = root.join("shell-approved.txt");
    let input = format!(
        "run command printf ok > {}\n/approve\n/exit\n",
        target.display()
    );

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
        .write_all(input.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert_eq!(fs::read_to_string(&target).unwrap(), "ok");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Shell command finished and verification was recorded."));
    assert!(stdout.contains("Pending Action"));
    assert!(stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("Command: printf ok >"));
    assert!(!stdout.contains("No proposed action is waiting for approval."));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_reject_after_shell_action_proposal_does_not_execute() {
    let root = smoke_root("slash-reject-shell");
    let target = root.join("shell-rejected.txt");
    let input = format!(
        "run command printf no > {}\n/reject\n/exit\n",
        target.display()
    );

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
        .write_all(input.as_bytes())
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(!target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Pending Action"));
    assert!(stdout.contains("Status: rejected"));
    assert!(stdout.contains("Command: printf no >"));
    assert!(!stdout.contains("Shell command finished and verification was recorded."));
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
    assert!(rejected_target.exists());
    assert!(approved_target.exists());
    assert_eq!(fs::read_to_string(&rejected_target).unwrap(), "");
    assert_eq!(fs::read_to_string(&approved_target).unwrap(), "");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> create file rejected.py"));
    assert!(!stdout.contains("Creating rejected.py."));
    assert!(stdout.contains(&format!(
        "Wrote {}.",
        fs::canonicalize(&rejected_target).unwrap().display()
    )));
    assert!(stdout.contains("No proposed action is waiting for rejection."));
    assert!(stdout.contains("No proposed action is waiting for approval."));
    assert!(stdout.contains("> create file approved.py"));
    assert!(!stdout.contains("Creating approved.py."));
    assert!(stdout.contains(&format!(
        "Wrote {}.",
        fs::canonicalize(&approved_target).unwrap().display()
    )));
    assert!(stdout.contains("Pending Action\nnone"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(!stdout.contains("Action: action-1 CreateFile"));
    assert!(!stdout.contains("Action: action-2 CreateFile"));
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
        .env("ELGAR_PROVIDER_CONFIG", "off")
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
