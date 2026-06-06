//! Reads submitted input from the inline terminal prompt.
//!
//! This file owns the event loop that waits for keyboard/paste input until the
//! user submits text or exits.

use std::io;

use crossterm::event;

use crate::{
    input::{TerminalInput, TerminalInputAction},
    terminal::{
        input::{keymap::handle_terminal_input_event, raw_mode::TerminalModeGuard},
        ui::prompt::InlinePromptRenderer,
        TerminalShellContext,
    },
};

/// Reads keyboard/paste events until the user submits text or exits.
///
/// Returns `Some(text)` for submitted input and `None` for terminal exit.
pub(crate) fn read_inline_prompt(
    context: &TerminalShellContext,
    initial_input: &str,
) -> io::Result<Option<String>> {
    let _guard = TerminalModeGuard::enter()?;
    let mut input = TerminalInput::from_text(initial_input);
    let mut renderer = InlinePromptRenderer::new(context.clone());
    renderer.render_with_cursor(input.text(), input.cursor())?;

    loop {
        match handle_terminal_input_event(event::read()?, &mut input) {
            TerminalInputAction::Continue => {
                renderer.render_with_cursor(input.text(), input.cursor())?
            }
            TerminalInputAction::Submit => {
                let submitted = input.drain().trim().to_string();
                renderer.clear()?;
                return Ok(Some(submitted));
            }
            TerminalInputAction::Exit => {
                renderer.clear()?;
                return Ok(None);
            }
        }
    }
}
