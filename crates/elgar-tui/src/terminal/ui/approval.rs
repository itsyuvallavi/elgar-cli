//! Pending approval display for terminal views.
//!
//! This renderer shows core-owned pending approval state. It does not approve,
//! deny, or execute anything.

use elgar_core::harness::PendingApproval;

use crate::terminal::ui::{
    approval_action::ApprovalAction, approval_card::render_pending_approval_card,
    prompt::terminal_width,
};

pub(in crate::terminal) fn render_pending_approval_text(approval: &PendingApproval) -> String {
    render_pending_approval_card(approval, terminal_width(), ApprovalAction::Approve).join("\n")
}

#[cfg(test)]
mod tests;
