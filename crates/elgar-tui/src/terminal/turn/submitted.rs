//! Handles submitted terminal input while the provider is idle.
//!
//! This file owns local slash-command execution and forwards plain text into
//! the harness-controlled provider-turn path.

use std::io;

use elgar_core::{
    harness::{approve_pending_approval, deny_pending_approval},
    logs::system::{append_log_event, LogInput, LogPhase},
    provider::ControllerProvider,
    session::Session,
};

use crate::{
    terminal::{
        commands::{
            clear_terminal_conversation, clear_visible_terminal,
            copy_conversation_to_terminal_clipboard, copy_raw_details_to_terminal_clipboard,
            parse_terminal_command, render_terminal_help, render_unknown_command, TerminalCommand,
        },
        input::keymap::{
            normalize_terminal_provider_text_input, terminal_text_should_run_inline_provider_text,
        },
        turn::provider::run_inline_provider_text_turn,
        ui::render::{print_and_record_local, print_new_conversation_lines, print_plain_block},
    },
    TuiShell,
};

/// Executes one submitted prompt while the provider is idle.
///
/// Local slash commands are handled here. Plain text is forwarded to the
/// harness-controlled provider turn.
pub(crate) fn handle_inline_submission<P>(
    submitted: &str,
    provider: &P,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<(bool, String)>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let turn_id = session.next_turn_id();
    let command = parse_terminal_command(submitted);
    let _ = append_log_event(
        &session.project_root,
        &session.id,
        LogInput::new(
            turn_id,
            LogPhase::Input,
            file!(),
            "handle_inline_submission",
            "input_classified",
        )
        .with_metadata(serde_json::json!({
            "submitted_chars": submitted.chars().count(),
            "classification": terminal_command_name(&command)
        })),
    );
    match command {
        TerminalCommand::Empty => Ok((false, String::new())),
        TerminalCommand::Exit => Ok((true, String::new())),
        TerminalCommand::Help => {
            print_and_record_local(shell, render_terminal_help())?;
            Ok((false, String::new()))
        }
        TerminalCommand::Clear => {
            session.reset_conversation();
            clear_terminal_conversation(shell);
            clear_visible_terminal()?;
            Ok((false, String::new()))
        }
        TerminalCommand::Copy => {
            let mut sink = io::stdout();
            let _ = copy_conversation_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            Ok((false, String::new()))
        }
        TerminalCommand::CopyRaw => {
            let mut sink = io::stdout();
            let _ = copy_raw_details_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            Ok((false, String::new()))
        }
        TerminalCommand::Cancel => {
            print_and_record_local(shell, "No provider request is running.")?;
            Ok((false, String::new()))
        }
        TerminalCommand::Approve => {
            let message = match approve_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => error.to_string(),
            };
            print_and_record_local(shell, message)?;
            Ok((false, String::new()))
        }
        TerminalCommand::Deny => {
            let message = match deny_pending_approval(session) {
                Ok(result) => result.message,
                Err(error) => error.to_string(),
            };
            print_and_record_local(shell, message)?;
            Ok((false, String::new()))
        }
        TerminalCommand::DetailsLast => {
            let before = shell.conversation.render_lines_with_styles().len();
            shell.push_latest_raw_details();
            print_new_conversation_lines(shell, before, false, false)?;
            Ok((false, String::new()))
        }
        TerminalCommand::Unknown(command) => {
            print_and_record_local(shell, render_unknown_command(command))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Text(text) => {
            if terminal_text_should_run_inline_provider_text(text) {
                let provider_input = normalize_terminal_provider_text_input(text);
                let preserved_input =
                    run_inline_provider_text_turn(&provider_input, provider, session, shell)?;
                Ok((false, preserved_input))
            } else {
                Ok((false, String::new()))
            }
        }
    }
}

fn terminal_command_name(command: &TerminalCommand<'_>) -> &'static str {
    match command {
        TerminalCommand::Empty => "empty",
        TerminalCommand::Exit => "exit",
        TerminalCommand::Help => "help",
        TerminalCommand::Clear => "clear",
        TerminalCommand::Copy => "copy",
        TerminalCommand::CopyRaw => "copy_raw",
        TerminalCommand::Cancel => "cancel",
        TerminalCommand::Approve => "approve",
        TerminalCommand::Deny => "deny",
        TerminalCommand::DetailsLast => "details_last",
        TerminalCommand::Unknown(_) => "unknown",
        TerminalCommand::Text(_) => "plain_text",
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    use elgar_core::{
        event::ProviderOutput,
        provider::{
            ChatMessage, ChatToolCall, ChatToolCallFunction, ChatToolDefinition,
            ControllerProvider, ProviderError, ProviderRequestMetadata,
        },
    };

    use super::*;

    #[test]
    fn approve_command_executes_pending_write_request() {
        let root = test_root("approve-write");
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("tui-approve-write", &root, &root);
        let provider = ToolCallProvider::write_file("demo.txt", "hello");
        let mut shell = TuiShell::new();

        shell.submit_harness_input(&provider, &mut session, "create demo.txt");
        assert_eq!(
            session
                .pending_approval()
                .map(|approval| approval.tool.as_str()),
            Some("write")
        );

        let result = handle_inline_submission("/approve", &provider, &mut session, &mut shell)
            .expect("approve command should run");

        assert_eq!(result, (false, String::new()));
        assert_eq!(fs::read_to_string(root.join("demo.txt")).unwrap(), "hello");
        assert!(session.pending_approval().is_none());
        assert!(shell
            .conversation_copy_text()
            .contains("VERIFIED_WRITE_EXECUTION"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn deny_command_clears_pending_write_request_without_execution() {
        let root = test_root("deny-write");
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("tui-deny-write", &root, &root);
        let provider = ToolCallProvider::write_file("demo.txt", "hello");
        let mut shell = TuiShell::new();

        shell.submit_harness_input(&provider, &mut session, "create demo.txt");
        assert!(session.pending_approval().is_some());

        let result = handle_inline_submission("/deny", &provider, &mut session, &mut shell)
            .expect("deny command should run");

        assert_eq!(result, (false, String::new()));
        assert!(!root.join("demo.txt").exists());
        assert!(session.pending_approval().is_none());
        assert!(shell.conversation_copy_text().contains("Denied approval-1"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clear_command_resets_core_session_and_visible_conversation() {
        let root = test_root("clear-session");
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("terminal-tui-session", &root, &root);
        let provider = ToolCallProvider::text_response("hello back");
        let mut shell = TuiShell::new();

        shell.submit_harness_input(&provider, &mut session, "hello");
        assert!(!session.events().is_empty());

        let result = handle_inline_submission("/clear", &provider, &mut session, &mut shell)
            .expect("clear command should run");

        assert_eq!(result, (false, String::new()));
        assert!(session.events().is_empty());
        assert_eq!(session.id, "terminal-tui-session-clear-1");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn approve_command_without_pending_request_stays_local() {
        let root = test_root("approve-empty");
        fs::create_dir_all(&root).unwrap();
        let mut session = Session::new("tui-approve-empty", &root, &root);
        let provider = ToolCallProvider::empty();
        let mut shell = TuiShell::new();

        let result = handle_inline_submission("/approve", &provider, &mut session, &mut shell)
            .expect("approve command should stay local");

        assert_eq!(result, (false, String::new()));
        assert!(shell
            .conversation_copy_text()
            .contains("No pending approval."));
        let _ = fs::remove_dir_all(root);
    }

    #[derive(Clone)]
    struct ToolCallProvider {
        outputs: Arc<Mutex<Vec<ProviderOutput>>>,
    }

    impl ToolCallProvider {
        fn empty() -> Self {
            Self {
                outputs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn text_response(message: &str) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(vec![ProviderOutput::new(message)])),
            }
        }

        fn write_file(path: &str, content: &str) -> Self {
            Self {
                outputs: Arc::new(Mutex::new(vec![
                    ProviderOutput::new("requesting write").with_tool_calls(vec![ChatToolCall {
                        id: "call-write".to_string(),
                        tool_type: "function".to_string(),
                        function: ChatToolCallFunction {
                            name: "write".to_string(),
                            arguments: serde_json::json!({
                                "path": path,
                                "content": content
                            })
                            .to_string(),
                        },
                    }]),
                    ProviderOutput::new("write is pending approval"),
                ])),
            }
        }
    }

    impl ControllerProvider for ToolCallProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("stub", None, "stub-request")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused"))
        }

        fn chat_messages_with_tools_with_metadata(
            &self,
            _messages: Vec<ChatMessage>,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            Ok(self.outputs.lock().expect("outputs lock").remove(0))
        }
    }

    fn test_root(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("elgar-tui-submitted-{name}-{}", std::process::id()))
    }
}
