//! Tests for terminal input editing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::super::{TerminalInput, TerminalInputAction};

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
fn shift_enter_inserts_newline_without_submitting() {
    let mut input = TerminalInput::from_text("first");

    assert_eq!(
        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
        TerminalInputAction::Continue
    );
    input.handle_key(key(KeyCode::Char('s')));

    assert_eq!(input.text(), "first\ns");
}

#[test]
fn alternate_newline_chords_insert_newline_without_submitting() {
    let mut input = TerminalInput::from_text("a");

    assert_eq!(
        input.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
        TerminalInputAction::Continue
    );
    assert_eq!(
        input.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)),
        TerminalInputAction::Continue
    );
    assert_eq!(
        input.handle_key(key(KeyCode::Char('\n'))),
        TerminalInputAction::Continue
    );

    assert_eq!(input.text(), "a\n\n\n");
}

#[test]
fn pasted_multiline_text_inserts_without_submitting() {
    let mut input = TerminalInput::from_text("before ");

    input.insert_text("line one\nline two");

    assert_eq!(input.text(), "before line one\nline two");
    assert_eq!(
        input.handle_key(key(KeyCode::Enter)),
        TerminalInputAction::Submit
    );
    assert_eq!(input.drain(), "before line one\nline two");
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
