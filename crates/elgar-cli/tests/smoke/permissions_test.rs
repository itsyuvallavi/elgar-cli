//! Scripted TUI smoke tests for non-mutating plain requests.

use std::{fs, io::Write};

use super::support::{force_stub_env, scripted_tui_command, smoke_root};

#[test]
fn tui_command_plain_file_request_and_reject_do_not_write_without_tool_result() {
    let root = smoke_root("plain-file-reject");
    let target = root.join("rejected.py");
    let mut child = force_stub_env(scripted_tui_command().arg("").current_dir(&root))
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
    assert!(stdout.contains("stub provider response"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("No pending approval."));
    assert!(!stdout.contains("Wrote "));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_plain_file_request_and_approve_do_not_write_without_tool_result() {
    let root = smoke_root("plain-file-approve");
    let target = root.join("approved.py");
    let mut command = scripted_tui_command();
    command.current_dir(&root);
    let mut child = force_stub_env(&mut command).spawn().unwrap();

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"create file approved.py\n/approve\n/exit\n")
        .unwrap();

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(!target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stub provider response"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("No pending approval."));
    assert!(!stdout.contains("Wrote "));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_plain_shell_text_and_approve_do_not_execute_without_tool_result() {
    let root = smoke_root("plain-shell-approve");
    let target = root.join("shell-approved.txt");
    let input = format!(
        "run command printf ok > {}\n/approve\n/exit\n",
        target.display()
    );
    let mut command = scripted_tui_command();
    command.current_dir(&root);
    let mut child = force_stub_env(&mut command).spawn().unwrap();

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
    assert!(stdout.contains("stub provider response"));
    assert!(stdout.contains("No pending approval."));
    assert!(!stdout.contains("Shell command finished and verification was recorded."));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_plain_shell_text_and_reject_do_not_execute_without_tool_result() {
    let root = smoke_root("plain-shell-reject");
    let target = root.join("shell-rejected.txt");
    let input = format!(
        "run command printf no > {}\n/reject\n/exit\n",
        target.display()
    );
    let mut command = scripted_tui_command();
    command.current_dir(&root);
    let mut child = force_stub_env(&mut command).spawn().unwrap();

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
    assert!(stdout.contains("stub provider response"));
    assert!(stdout.contains("No pending approval."));
    assert!(!stdout.contains("Status: rejected"));
    assert!(!stdout.contains("Shell command finished and verification was recorded."));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_command_line_loop_keeps_plain_requests_non_mutating() {
    let root = smoke_root("line-loop-plain-non-mutating");
    let rejected_target = root.join("rejected.py");
    let approved_target = root.join("approved.py");
    let mut command = scripted_tui_command();
    command.current_dir(&root);
    let mut child = force_stub_env(&mut command).spawn().unwrap();

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
    assert!(!approved_target.exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("> create file rejected.py"));
    assert!(stdout.contains("stub provider response"));
    assert!(!stdout.contains("Wrote "));
    assert!(stdout.contains("No pending approval."));
    assert!(stdout.contains("> create file approved.py"));
    assert!(!stdout.contains("Status: applied and verified"));
    assert!(!stdout.contains("Action: action-1 CreateFile"));
    assert!(!stdout.contains("Action: action-2 CreateFile"));
    assert!(stdout.contains("Exiting Elgar TUI."));
    assert!(!stdout.contains("Input was not recognized"));
    assert!(!stdout.contains("lm-studio"));
    assert!(!stdout.contains("LM Studio smoke failed"));

    let _ = fs::remove_dir_all(root);
}
