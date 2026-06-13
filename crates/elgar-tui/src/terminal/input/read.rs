//! Reads submitted input from the inline terminal prompt.
//!
//! This file owns the event loop that waits for keyboard/paste input until the
//! user submits text or exits.

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::{
    input::{TerminalInput, TerminalInputAction},
    terminal::{
        input::{keymap::handle_terminal_input_event, raw_mode::TerminalModeGuard},
        ui::{approval_action::ApprovalAction, prompt::InlinePromptRenderer},
        TerminalShellContext,
    },
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InlinePromptSubmission {
    Text(String),
    Approval(ApprovalAction),
}

/// Reads keyboard/paste events until the user submits text or exits.
///
/// Returns `Some(text)` for submitted input and `None` for terminal exit.
pub(crate) fn read_inline_prompt(
    context: &TerminalShellContext,
    initial_input: &str,
) -> io::Result<Option<InlinePromptSubmission>> {
    let _guard = TerminalModeGuard::enter()?;
    let mut input = TerminalInput::from_text(initial_input);
    let mut approval_action = ApprovalAction::Approve;
    let mut renderer = InlinePromptRenderer::new(context.clone());
    renderer.render_with_cursor(input.text(), input.cursor())?;

    loop {
        let event = event::read()?;
        if context.approval_tool.is_some() && input.text().is_empty() {
            match approval_action_for_event(&event, approval_action) {
                ApprovalPromptEvent::Continue => {}
                ApprovalPromptEvent::Select(action) => {
                    approval_action = action;
                    renderer.set_context(
                        context
                            .clone()
                            .with_approval_action_selected(approval_action),
                    );
                    renderer.render_with_cursor(input.text(), input.cursor())?;
                    continue;
                }
                ApprovalPromptEvent::Submit => {
                    renderer.clear()?;
                    return Ok(Some(InlinePromptSubmission::Approval(approval_action)));
                }
            }
        }

        match handle_terminal_input_event(event, &mut input) {
            TerminalInputAction::Continue => {
                renderer.render_with_cursor(input.text(), input.cursor())?
            }
            TerminalInputAction::Submit => {
                let submitted = input.drain().trim().to_string();
                renderer.clear()?;
                return Ok(Some(InlinePromptSubmission::Text(submitted)));
            }
            TerminalInputAction::Exit => {
                renderer.clear()?;
                return Ok(None);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApprovalPromptEvent {
    Continue,
    Select(ApprovalAction),
    Submit,
}

fn approval_action_for_event(event: &Event, selected: ApprovalAction) -> ApprovalPromptEvent {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Tab | KeyCode::Left | KeyCode::Right => {
                ApprovalPromptEvent::Select(selected.toggled())
            }
            KeyCode::Enter => ApprovalPromptEvent::Submit,
            _ => ApprovalPromptEvent::Continue,
        },
        _ => ApprovalPromptEvent::Continue,
    }
}
