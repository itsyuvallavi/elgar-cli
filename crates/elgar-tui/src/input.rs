use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TerminalInput {
    text: String,
    cursor: usize,
}

impl TerminalInput {
    pub(crate) fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn drain(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

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
                    self.cursor = floor_char_boundary(&self.text, self.cursor);
                    self.text.insert(self.cursor, character);
                    self.cursor += character.len_utf8();
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
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{TerminalInput, TerminalInputAction};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn terminal_input_edits_a_single_line() {
        let mut input = TerminalInput::default();

        input.handle_key(key(KeyCode::Char('a')));
        input.handle_key(key(KeyCode::Char('c')));
        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Char('b')));

        assert_eq!(input.text(), "abc");

        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.text(), "ac");

        input.handle_key(key(KeyCode::Right));
        input.handle_key(key(KeyCode::Char('d')));
        assert_eq!(input.text(), "acd");
    }

    #[test]
    fn terminal_input_moves_by_characters_for_multibyte_text() {
        let mut input = TerminalInput::from_text("a🙂b");

        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.text(), "ab");

        input.handle_key(key(KeyCode::Char('é')));
        assert_eq!(input.text(), "aéb");
    }

    #[test]
    fn terminal_input_delete_removes_multibyte_character_at_cursor() {
        let mut input = TerminalInput::from_text("a🙂b");

        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Left));
        input.handle_key(key(KeyCode::Delete));

        assert_eq!(input.text(), "ab");
    }

    #[test]
    fn enter_submits_and_drain_clears_input() {
        let mut input = TerminalInput::default();

        input.handle_key(key(KeyCode::Char('h')));
        input.handle_key(key(KeyCode::Char('i')));

        assert_eq!(
            input.handle_key(key(KeyCode::Enter)),
            TerminalInputAction::Submit
        );
        assert_eq!(input.drain(), "hi");
        assert_eq!(input.text(), "");
    }

    #[test]
    fn escape_clears_input_and_ctrl_c_ctrl_d_exit() {
        let mut input = TerminalInput::default();

        input.handle_key(key(KeyCode::Char('h')));
        input.handle_key(key(KeyCode::Char('i')));
        assert_eq!(
            input.handle_key(key(KeyCode::Esc)),
            TerminalInputAction::Continue
        );
        assert_eq!(input.text(), "");
        assert_eq!(
            input.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            TerminalInputAction::Exit
        );
        assert_eq!(
            input.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            TerminalInputAction::Exit
        );
    }
}
