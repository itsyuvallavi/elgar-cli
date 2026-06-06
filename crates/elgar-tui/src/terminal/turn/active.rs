//! Active provider request input handling.
//!
//! While the model is responding, the user can type the next prompt or submit
//! `/cancel`. This file owns that provider-running input behavior.

use std::io;

use crossterm::event;

use crate::{
    input::TerminalInput,
    terminal::{
        input::keymap::{handle_active_provider_input_event, ActiveProviderKeyAction},
        turn::provider_worker::ProviderTurnTask,
        ui::prompt::{InlineWorkingRenderer, LiveProviderOutput},
    },
};

/// Handles keyboard input while a provider request is already running.
///
/// This lets the user type the next prompt or submit `/cancel` without waiting
/// for the model request to finish.
pub(super) fn handle_active_provider_event(
    task: &ProviderTurnTask,
    input: &mut TerminalInput,
    working: &mut InlineWorkingRenderer,
    tick: usize,
    elapsed_secs: u64,
    live_output: &LiveProviderOutput,
) -> io::Result<()> {
    match handle_active_provider_input_event(event::read()?, input) {
        ActiveProviderKeyAction::Continue => working.render_with_cursor(
            tick,
            elapsed_secs,
            input.text(),
            input.cursor(),
            live_output,
        ),
        ActiveProviderKeyAction::Cancel => {
            task.cancel();
            working.render_with_cursor(
                tick,
                elapsed_secs,
                input.text(),
                input.cursor(),
                live_output,
            )
        }
        ActiveProviderKeyAction::Exit => {
            task.cancel();
            Ok(())
        }
    }
}
