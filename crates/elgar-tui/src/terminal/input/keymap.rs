//! Keyboard event routing for the inline prompt.
//!
//! This file translates crossterm key events into local prompt actions.

use crossterm::event::{Event, KeyEvent, KeyEventKind};

use crate::{
    input::{TerminalInput, TerminalInputAction},
    terminal::{
        commands::{parse_terminal_command, TerminalCommand},
        input::normalization::normalize_pasted_transcript_input,
    },
};

/// Handle keyboard input while no provider request is active.
pub(crate) fn handle_terminal_input_event(
    event: crossterm::event::Event,
    input: &mut TerminalInput,
) -> TerminalInputAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => input.handle_key(key),
        Event::Paste(text) => {
            input.insert_text(&text);
            TerminalInputAction::Continue
        }
        _ => TerminalInputAction::Continue,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveProviderKeyAction {
    Continue,
    Cancel,
    Exit,
}

/// Interpret one key press while a provider request is active.
pub(crate) fn handle_active_provider_key(
    key: KeyEvent,
    input: &mut TerminalInput,
) -> ActiveProviderKeyAction {
    match input.handle_key(key) {
        TerminalInputAction::Continue => ActiveProviderKeyAction::Continue,
        TerminalInputAction::Submit => {
            if matches!(
                parse_terminal_command(input.text()),
                TerminalCommand::Cancel
            ) {
                let _ = input.drain();
                ActiveProviderKeyAction::Cancel
            } else {
                ActiveProviderKeyAction::Continue
            }
        }
        TerminalInputAction::Exit => ActiveProviderKeyAction::Exit,
    }
}

/// Handle keyboard/paste input while a provider request is active.
pub(crate) fn handle_active_provider_input_event(
    event: crossterm::event::Event,
    input: &mut TerminalInput,
) -> ActiveProviderKeyAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_active_provider_key(key, input)
        }
        Event::Paste(text) => {
            input.insert_text(&text);
            ActiveProviderKeyAction::Continue
        }
        _ => ActiveProviderKeyAction::Continue,
    }
}

pub(crate) fn terminal_text_should_run_inline_provider_text(_text: &str) -> bool {
    true
}

pub(crate) fn normalize_terminal_provider_text_input(text: &str) -> String {
    normalize_pasted_transcript_input(text).trim().to_string()
}
