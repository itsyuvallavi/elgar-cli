//! Tests for local terminal command messages.

use super::super::render_terminal_help;

#[test]
fn help_lists_active_harness_commands() {
    let help = render_terminal_help();

    assert!(help.starts_with("Commands\nChat"));
    assert!(help.contains("plain text"));
    assert!(help.contains("harness-controlled"));
    assert!(help.contains("/cancel"));
    assert!(help.contains("/approve"));
    assert!(help.contains("/deny"));
    assert!(help.contains("/reject"));
    assert!(help.contains("/details last"));
    assert!(help.contains("/copy raw"));
    assert!(help.contains("/exit"));
    assert!(!help.contains("/raw <prompt>"));
}
