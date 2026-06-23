//! Transcript rendering helpers for scripted TUI mode.

use elgar_core::session::Session;
use elgar_tui::{terminal::render_pending_approval_text, TuiShell};

pub(super) fn render_tui_turn(shell: &TuiShell, session: &Session) -> String {
    let mut rendered = shell.render_scripted_transcript();
    if let Some(approval) = session.pending_approval() {
        if !rendered.is_empty() {
            rendered.push('\n');
        }
        rendered.push_str(&render_pending_approval_text(approval));
    }
    rendered
}
