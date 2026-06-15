//! Handles submitted terminal input while the provider is idle.
//!
//! This file owns local slash-command execution and forwards plain text into
//! the harness-controlled provider-turn path.

mod commands;
mod logging;

#[cfg(test)]
mod tests;

use std::io;

use elgar_core::{provider::ControllerProvider, session::Session};

use crate::{
    terminal::{
        commands::{parse_terminal_command, TerminalCommand},
        input::keymap::{
            normalize_terminal_provider_text_input, terminal_text_should_run_inline_provider_text,
        },
        turn::provider::run_inline_provider_text_turn,
    },
    TuiShell,
};

use commands::handle_terminal_command;
use logging::log_input_classified;

/// Executes one submitted prompt while the provider is idle.
///
/// Local slash commands are handled here. Plain text is forwarded to the
/// harness-controlled provider turn.
pub(crate) fn handle_inline_submission<P>(
    submitted: &str,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<(bool, String)>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let turn_id = session.next_turn_id();
    let command = parse_terminal_command(submitted);
    log_input_classified(session, turn_id, submitted, &command);

    if let Some(result) = handle_terminal_command(&command, provider, session, shell)? {
        return Ok(result);
    }

    match command {
        TerminalCommand::Text(text) => {
            if terminal_text_should_run_inline_provider_text(text) {
                let provider_input = normalize_terminal_provider_text_input(text);
                let preserved_input =
                    run_inline_provider_text_turn(&provider_input, provider, session, shell)?;
                Ok((false, preserved_input))
            } else {
                Ok((false, String::new()))
            }
        }
        _ => Ok((false, String::new())),
    }
}
