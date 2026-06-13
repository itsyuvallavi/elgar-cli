//! Tests for inline prompt approval keyboard actions.

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::{approval_action_for_event, ApprovalPromptEvent};
use crate::terminal::ui::approval_action::ApprovalAction;

#[test]
fn tab_toggles_selected_approval_action() {
    let event = key_event(KeyCode::Tab);

    let action = approval_action_for_event(&event, ApprovalAction::Approve);

    assert_eq!(action, ApprovalPromptEvent::Select(ApprovalAction::Deny));
}

#[test]
fn arrows_toggle_selected_approval_action() {
    let left = approval_action_for_event(&key_event(KeyCode::Left), ApprovalAction::Approve);
    let right = approval_action_for_event(&key_event(KeyCode::Right), ApprovalAction::Deny);

    assert_eq!(left, ApprovalPromptEvent::Select(ApprovalAction::Deny));
    assert_eq!(right, ApprovalPromptEvent::Select(ApprovalAction::Approve));
}

#[test]
fn enter_submits_selected_approval_action() {
    let action = approval_action_for_event(&key_event(KeyCode::Enter), ApprovalAction::Approve);

    assert_eq!(action, ApprovalPromptEvent::Submit);
}

fn key_event(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
