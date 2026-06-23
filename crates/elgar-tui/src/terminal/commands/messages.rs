//! User-facing text for local terminal commands.
//!
//! These messages are local UI text, not model-authored chat answers.

pub fn render_terminal_help() -> &'static str {
    "Commands\nChat\n  plain text                    Send one harness-controlled model turn\n  /cancel                       Cancel the active provider turn\nApproval\n  /approve                      Approve and execute the pending risky primitive\n  /approve continue             Approve, execute, then continue the task\n  /deny                         Deny the pending risky primitive\n  /reject                       Deny the pending risky primitive\nPermissions\n  /permissions                  Show current permission mode\n  /permissions review_all       Require approval for write, edit, and bash\n  /permissions workspace_write  Auto-run safe relative writes in the launch folder\n  /permissions full_access      Auto-run trusted launch-folder writes, edits, and bash\nView\n  /clear                        Clear the visible conversation\n  /new                          Clear the visible conversation\n  /details last                 Show latest hidden details\n  /copy                         Copy the conversation\n  /copy raw                     Copy hidden details\n  /help                         Show commands\n  /commands                     Show commands\nExit\n  /exit                         Quit\n  /quit                         Quit\n  /q                            Quit"
}

pub fn render_unknown_command(command: &str) -> String {
    format!(
        "Unknown command: {command}\nUse /commands to see local commands. Plain text without / is sent to the model."
    )
}
