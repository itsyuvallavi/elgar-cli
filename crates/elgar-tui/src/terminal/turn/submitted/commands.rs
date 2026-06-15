//! Local terminal command handling for submitted input.

use std::io;

use elgar_core::{
    harness::{approve_pending_approval, deny_pending_approval, PermissionMode},
    provider::ControllerProvider,
    session::Session,
};

use crate::{
    terminal::{
        commands::{
            clear_terminal_conversation, clear_visible_terminal,
            copy_conversation_to_terminal_clipboard, copy_raw_details_to_terminal_clipboard,
            render_terminal_help, render_unknown_command, TerminalCommand,
        },
        turn::provider::run_inline_provider_text_turn,
        ui::render::{print_and_record_local, print_new_conversation_lines, print_plain_block},
    },
    TuiShell,
};

const APPROVAL_CONTINUATION_PROMPT: &str = "The approved action has executed. Continue the user's current task using verified session facts and current tools. Inspect if needed. Do not claim completion until verified.";

pub(super) fn handle_terminal_command<P>(
    command: &TerminalCommand<'_>,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<Option<(bool, String)>>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let handled = match command {
        TerminalCommand::Empty => (false, String::new()),
        TerminalCommand::Exit => (true, String::new()),
        TerminalCommand::Help => {
            print_and_record_local(shell, render_terminal_help())?;
            (false, String::new())
        }
        TerminalCommand::Clear => {
            session.reset_conversation();
            clear_terminal_conversation(shell);
            clear_visible_terminal()?;
            (false, String::new())
        }
        TerminalCommand::Copy => {
            let mut sink = io::stdout();
            let _ = copy_conversation_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            (false, String::new())
        }
        TerminalCommand::CopyRaw => {
            let mut sink = io::stdout();
            let _ = copy_raw_details_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            (false, String::new())
        }
        TerminalCommand::Cancel => {
            print_and_record_local(shell, "No provider request is running.")?;
            (false, String::new())
        }
        TerminalCommand::Approve => {
            let message = match approve_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => error.to_string(),
            };
            print_and_record_local(shell, message)?;
            (false, String::new())
        }
        TerminalCommand::ApproveContinue => {
            let message = match approve_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => {
                    print_and_record_local(shell, error.to_string())?;
                    return Ok(Some((false, String::new())));
                }
            };
            print_and_record_local(shell, message)?;
            let preserved = run_inline_provider_text_turn(
                APPROVAL_CONTINUATION_PROMPT,
                provider,
                session,
                shell,
            )?;
            (false, preserved)
        }
        TerminalCommand::Deny => {
            let message = match deny_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => error.to_string(),
            };
            print_and_record_local(shell, message)?;
            (false, String::new())
        }
        TerminalCommand::Permissions(mode) => {
            let message = handle_permissions_command(session, mode);
            print_and_record_local(shell, message)?;
            (false, String::new())
        }
        TerminalCommand::DetailsLast => {
            let before = shell.conversation.render_lines_with_styles().len();
            shell.push_latest_raw_details();
            print_new_conversation_lines(shell, before, false, false)?;
            (false, String::new())
        }
        TerminalCommand::Unknown(command) => {
            print_and_record_local(shell, render_unknown_command(command))?;
            (false, String::new())
        }
        TerminalCommand::Text(_) => return Ok(None),
    };
    Ok(Some(handled))
}

fn handle_permissions_command(session: &mut Session, mode: &str) -> String {
    match mode {
        "" => format!("Permission mode: {}", session.permission_mode().as_str()),
        "review_all" => {
            session.set_permission_mode(PermissionMode::ReviewAll);
            "Permission mode set to review_all.".to_string()
        }
        "workspace_write" => {
            session.set_permission_mode(PermissionMode::WorkspaceWrite);
            "Permission mode set to workspace_write. Safe relative writes inside the launch folder can run without approval; bash, edit, absolute paths, and parent paths still require approval.".to_string()
        }
        "full_access" => {
            session.set_permission_mode(PermissionMode::FullAccess);
            "Permission mode set to full_access. Trusted launch-folder writes, edits, and bash can run without approval; unsafe paths remain rejected by execution checks.".to_string()
        }
        _ => {
            "Unknown permission mode. Use /permissions review_all, /permissions workspace_write, or /permissions full_access."
                .to_string()
        }
    }
}
