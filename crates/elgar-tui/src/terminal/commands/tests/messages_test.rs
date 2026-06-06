//! Tests for local terminal command messages.

use super::super::render_terminal_help;

#[test]
fn help_lists_active_raw_only_commands() {
    let help = render_terminal_help();

    assert!(help.starts_with("Commands\nChat"));
    assert!(help.contains("/raw <prompt>"));
    assert!(help.contains("/cancel"));
    assert!(help.contains("/details last"));
    assert!(help.contains("/copy raw"));
    assert!(help.contains("/exit"));
}
