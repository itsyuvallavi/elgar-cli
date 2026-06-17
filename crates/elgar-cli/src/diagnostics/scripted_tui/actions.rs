//! Scripted TUI command actions.

use elgar_core::{
    harness::{approve_pending_approval, deny_pending_approval, PermissionMode},
    provider::ControllerProvider,
    session::Session,
};
use elgar_tui::terminal::TerminalCommand;

use super::commands::{parse_scripted_command, render_tui_unknown_command};

const APPROVAL_CONTINUATION_PROMPT: &str = "The approved action has executed. Continue the user's current task using verified session facts and current tools. Inspect if needed. Do not claim completion until verified.";

/// Applies one submitted scripted input to the shell/session.
pub(super) fn submit_tui_input<P>(
    shell: &mut elgar_tui::TuiShell,
    provider: &P,
    session: &mut Session,
    input: &str,
) where
    P: ControllerProvider,
{
    match parse_scripted_command(input) {
        TerminalCommand::Cancel => {
            shell.push_local_message("No active provider turn to cancel.");
        }
        TerminalCommand::Approve => approve_scripted(shell, session),
        TerminalCommand::ApproveContinue => approve_continue_scripted(shell, provider, session),
        TerminalCommand::Deny => deny_scripted(shell, session),
        TerminalCommand::Permissions(mode) => set_permissions_scripted(shell, session, mode),
        TerminalCommand::DetailsLast => {
            shell.push_latest_raw_details();
        }
        TerminalCommand::Unknown(command) => {
            shell.push_local_message(render_tui_unknown_command(command));
        }
        TerminalCommand::Text(text) => {
            shell.submit_harness_input(provider, session, text);
        }
        TerminalCommand::Clear => {
            session.reset_conversation();
            shell.clear_conversation();
        }
        TerminalCommand::Empty
        | TerminalCommand::Help
        | TerminalCommand::Copy
        | TerminalCommand::CopyRaw
        | TerminalCommand::Exit => {}
    }
}

pub(super) fn set_permissions_scripted(
    shell: &mut elgar_tui::TuiShell,
    session: &mut Session,
    mode: &str,
) {
    match mode {
        "" => shell.push_local_message(format!(
            "Permission mode: {}",
            session.permission_mode().as_str()
        )),
        "review_all" => {
            session.set_permission_mode(PermissionMode::ReviewAll);
            shell.push_local_message("Permission mode set to review_all.");
        }
        "workspace_write" => {
            session.set_permission_mode(PermissionMode::WorkspaceWrite);
            shell.push_local_message("Permission mode set to workspace_write. Safe relative writes inside the launch folder can run without approval; bash, edit, absolute paths, and parent paths still require approval.");
        }
        "full_access" => {
            session.set_permission_mode(PermissionMode::FullAccess);
            shell.push_local_message("Permission mode set to full_access. Trusted launch-folder writes, edits, and bash can run without approval; unsafe paths remain rejected by execution checks.");
        }
        _ => shell.push_local_message(
            "Unknown permission mode. Use /permissions review_all, /permissions workspace_write, or /permissions full_access.",
        ),
    }
}

pub(super) fn approve_scripted(shell: &mut elgar_tui::TuiShell, session: &mut Session) {
    match approve_pending_approval(session) {
        Ok(result) => {
            shell.push_execution_result_message(result.message);
        }
        Err(error) => shell.push_local_message(error.to_string()),
    }
}

pub(super) fn approve_continue_scripted<P>(
    shell: &mut elgar_tui::TuiShell,
    provider: &P,
    session: &mut Session,
) where
    P: ControllerProvider,
{
    match approve_pending_approval(session) {
        Ok(result) => {
            shell.push_execution_result_message(result.message);
            shell.submit_harness_input(provider, session, APPROVAL_CONTINUATION_PROMPT);
        }
        Err(error) => shell.push_local_message(error.to_string()),
    }
}

pub(super) fn deny_scripted(shell: &mut elgar_tui::TuiShell, session: &mut Session) {
    match deny_pending_approval(session) {
        Ok(result) => shell.push_local_message(result.message),
        Err(error) => shell.push_local_message(error.to_string()),
    }
}
