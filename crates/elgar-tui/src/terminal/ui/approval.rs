//! Pending approval display for the terminal prompt.
//!
//! This renderer shows core-owned pending approval state. It does not approve,
//! deny, or execute anything.

use std::io;

use elgar_core::harness::PendingApproval;

use crate::terminal::ui::{
    approval_action::ApprovalAction,
    approval_card::{render_approval_footer_actions, render_pending_approval_card},
    prompt::terminal_width,
    render::print_plain_block,
};

/// Print the current pending approval card, if one exists.
pub(crate) fn print_pending_approval(approval: Option<&PendingApproval>) -> io::Result<()> {
    let Some(approval) = approval else {
        return Ok(());
    };

    print_plain_block(&render_pending_approval_text(approval))
}

pub(in crate::terminal) fn render_pending_approval_text(approval: &PendingApproval) -> String {
    render_pending_approval_card(approval, terminal_width(), ApprovalAction::Approve).join("\n")
}

pub(in crate::terminal) fn render_pending_approval_footer_actions(
    approval: &PendingApproval,
    selected: ApprovalAction,
) -> String {
    render_approval_footer_actions_for_tool(&approval.tool, selected)
}

pub(in crate::terminal) fn render_approval_footer_actions_for_tool(
    tool: &str,
    selected: ApprovalAction,
) -> String {
    render_approval_footer_actions(tool, selected)
}

#[cfg(test)]
mod tests;
