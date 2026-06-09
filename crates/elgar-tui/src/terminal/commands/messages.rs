//! User-facing text for local terminal commands.
//!
//! These messages are local UI text, not model-authored chat answers.

pub(crate) fn render_terminal_help() -> &'static str {
    "Commands\nChat\n  plain text           Send one harness-controlled model turn\n  /cancel              Cancel the active provider turn\nView\n  /clear               Clear the visible conversation\n  /new                 Clear the visible conversation\n  /details last        Show latest hidden details\n  /copy                Copy the conversation\n  /copy raw            Copy hidden details\n  /help                Show commands\n  /commands            Show commands\nExit\n  /exit                Quit\n  /quit                Quit\n  /q                   Quit"
}

pub(crate) fn render_unknown_command(command: &str) -> String {
    format!(
        "Unknown command: {command}\nUse /commands to see local commands. Plain text without / is sent to the model."
    )
}
