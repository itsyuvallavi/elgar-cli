//! Local terminal command handling for submitted input.

use std::io;

use elgar_core::{
    harness::{approve_pending_approval, deny_pending_approval},
    session::Session,
};

use crate::{
    terminal::{
        commands::{
            clear_terminal_conversation, clear_visible_terminal,
            copy_conversation_to_terminal_clipboard, copy_raw_details_to_terminal_clipboard,
            render_terminal_help, render_unknown_command, TerminalCommand,
        },
        ui::render::{print_and_record_local, print_new_conversation_lines, print_plain_block},
    },
    TuiShell,
};

pub(super) fn handle_terminal_command(
    command: &TerminalCommand<'_>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<Option<(bool, String)>> {
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
        TerminalCommand::Deny => {
            let message = match deny_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => error.to_string(),
            };
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
