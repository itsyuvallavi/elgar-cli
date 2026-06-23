//! ANSI styling for inline approval cards.
//!
//! The approval card content stays plain in `approval_card.rs`; this module
//! adds terminal color for the interactive inline prompt only.

use crate::terminal::{
    ui::approval_action::ApprovalAction, ANSI_BOLD, ANSI_CYAN, ANSI_EVENT, ANSI_MUTED, ANSI_RESET,
    ANSI_TEXT,
};

pub(crate) fn color_card_line(line: &str, selected: ApprovalAction) -> String {
    let mut line = if line.contains('╭') || line.contains('╰') {
        format!("{ANSI_CYAN}{ANSI_BOLD}{line}{ANSI_RESET}")
    } else if line.contains('│') {
        format!("{ANSI_TEXT}{line}{ANSI_RESET}")
    } else {
        line.to_string()
    };

    let approve = action_button("Approve", selected == ApprovalAction::Approve);
    let deny = action_button("Deny", selected == ApprovalAction::Deny);
    line = line.replace(
        &approve,
        &color_action(&approve, selected == ApprovalAction::Approve),
    );
    line.replace(
        &deny,
        &color_action(&deny, selected == ApprovalAction::Deny),
    )
}

fn action_button(label: &str, selected: bool) -> String {
    if selected {
        format!("[{label}]")
    } else {
        format!(" {label} ")
    }
}

fn color_action(label: &str, selected: bool) -> String {
    if selected {
        format!("{ANSI_EVENT}{ANSI_BOLD}{label}{ANSI_RESET}{ANSI_TEXT}")
    } else {
        format!("{ANSI_MUTED}{label}{ANSI_RESET}{ANSI_TEXT}")
    }
}
