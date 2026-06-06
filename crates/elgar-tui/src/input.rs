//! Editable terminal input state.
//!
//! This file owns the text buffer and cursor movement for the inline prompt.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TerminalInput {
    text: String,
    cursor: usize,
}

impl TerminalInput {
    /// Create an input buffer with the cursor at the end.
    pub(crate) fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn drain(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        self.cursor = floor_char_boundary(&self.text, self.cursor);
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Apply one keyboard event to the editable input buffer.
    pub(crate) fn handle_key(&mut self, key: KeyEvent) -> TerminalInputAction {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                TerminalInputAction::Exit
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                TerminalInputAction::Exit
            }
            KeyCode::Esc => {
                self.text.clear();
                self.cursor = 0;
                TerminalInputAction::Continue
            }
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.insert_text("\n");
                TerminalInputAction::Continue
            }
            KeyCode::Char('\n') => {
                self.insert_text("\n");
                TerminalInputAction::Continue
            }
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.insert_text("\n");
                TerminalInputAction::Continue
            }
            KeyCode::Enter => TerminalInputAction::Submit,
            KeyCode::Backspace => {
                let cursor = floor_char_boundary(&self.text, self.cursor);
                if cursor > 0 {
                    let previous = previous_char_boundary(&self.text, cursor);
                    self.text.drain(previous..cursor);
                    self.cursor = previous;
                }
                TerminalInputAction::Continue
            }
            KeyCode::Delete => {
                let cursor = floor_char_boundary(&self.text, self.cursor);
                if cursor < self.text.len() {
                    let next = next_char_boundary(&self.text, cursor);
                    self.text.drain(cursor..next);
                    self.cursor = cursor;
                }
                TerminalInputAction::Continue
            }
            KeyCode::Left => {
                self.cursor = previous_char_boundary(&self.text, self.cursor);
                TerminalInputAction::Continue
            }
            KeyCode::Right => {
                self.cursor = next_char_boundary(&self.text, self.cursor);
                TerminalInputAction::Continue
            }
            KeyCode::Home => {
                self.cursor = 0;
                TerminalInputAction::Continue
            }
            KeyCode::End => {
                self.cursor = self.text.len();
                TerminalInputAction::Continue
            }
            KeyCode::Char(character) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.insert_text(&character.to_string());
                }
                TerminalInputAction::Continue
            }
            _ => TerminalInputAction::Continue,
        }
    }
}

fn floor_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while cursor > 0 && !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

fn previous_char_boundary(text: &str, cursor: usize) -> usize {
    let cursor = floor_char_boundary(text, cursor);
    if cursor == 0 {
        return 0;
    }

    let mut previous = cursor - 1;
    while previous > 0 && !text.is_char_boundary(previous) {
        previous -= 1;
    }
    previous
}

fn next_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = floor_char_boundary(text, cursor);
    if cursor >= text.len() {
        return text.len();
    }

    cursor += 1;
    while cursor < text.len() && !text.is_char_boundary(cursor) {
        cursor += 1;
    }
    cursor
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalInputAction {
    Continue,
    Submit,
    Exit,
}

#[cfg(test)]
mod tests;
