//! Tests for the line-based scripted TUI mode.

use std::{
    fs,
    sync::{Arc, Mutex},
};

use elgar_core::{
    event::ProviderOutput,
    provider::{
        ChatMessage, ChatToolCall, ChatToolCallFunction, ChatToolDefinition, ControllerProvider,
        ProviderError, ProviderRequestMetadata,
    },
};

use crate::{
    is_tui_cancel_command, is_tui_clear_command, is_tui_copy_command, is_tui_copy_raw_command,
    is_tui_details_command, is_tui_exit_command, is_tui_help_command, render_tui_help,
    render_tui_script, run_tui_loop, run_tui_loop_with_runtime,
    should_launch_terminal_tui_by_default, tui_unknown_command, TUI_COMMAND, TUI_TERMINAL_COMMAND,
};

#[test]
fn default_terminal_launch_requires_interactive_stdio() {
    assert!(should_launch_terminal_tui_by_default(true, true));
    assert!(!should_launch_terminal_tui_by_default(false, true));
    assert!(!should_launch_terminal_tui_by_default(true, false));
    assert!(!should_launch_terminal_tui_by_default(false, false));
}

#[test]
fn terminal_tui_command_is_separate_from_line_loop_command() {
    assert_eq!(TUI_COMMAND, "tui");
    assert_eq!(TUI_TERMINAL_COMMAND, "tui-terminal");
}

#[test]
fn tui_exit_commands_are_explicit() {
    assert!(is_tui_exit_command("/exit"));
    assert!(is_tui_exit_command(" /quit "));
    assert!(is_tui_exit_command("/q"));
    assert!(!is_tui_exit_command("exit"));
}

#[test]
fn tui_supported_commands_are_explicit() {
    assert!(is_tui_help_command("/help"));
    assert!(is_tui_help_command(" /commands "));
    assert!(is_tui_copy_command("/copy"));
    assert!(is_tui_copy_raw_command("/copy raw"));
    assert!(is_tui_details_command("/details last"));
    assert!(is_tui_clear_command("/clear"));
    assert!(is_tui_clear_command(" /new "));
    assert!(is_tui_cancel_command("/cancel"));

    assert!(!is_tui_help_command("help"));
    assert!(!is_tui_copy_command("copy"));
    assert!(!is_tui_clear_command("clear"));
}

#[test]
fn tui_unknown_slash_command_is_local() {
    assert_eq!(tui_unknown_command("/model"), Some("/model"));
    assert_eq!(tui_unknown_command(" /settings "), Some("/settings"));
    assert_eq!(tui_unknown_command("/help"), None);
    assert_eq!(tui_unknown_command("/raw hello"), Some("/raw hello"));
    assert_eq!(tui_unknown_command("/details last"), None);
    assert_eq!(tui_unknown_command("/copy raw"), None);
    assert_eq!(tui_unknown_command("model"), None);
}

#[test]
fn tui_help_lists_supported_local_commands() {
    let help = render_tui_help();

    assert!(help.starts_with("Commands\nChat"));
    assert!(help.contains("harness-controlled"));
    assert!(help.contains("/cancel"));
    assert!(help.contains("/copy raw"));
    assert!(help.contains("/exit"));
    assert!(!help.contains("/raw <prompt>"));
    assert!(!help.contains("/tool"));
    assert!(!help.contains("/permissions"));
}

#[test]
fn tui_script_renders_default_stub_turns_and_stops_on_exit() {
    let rendered = render_tui_script(
        ["what does the harness do?", "/exit", "what should not run?"],
        ".",
        ".",
    );

    assert!(rendered.contains("> what does the harness do?"));
    assert!(rendered.contains("stub provider response"));
    assert!(!rendered.contains("what should not run?"));
    assert!(!rendered.contains("lm-studio"));
}

#[test]
fn tui_script_raw_detail_commands_are_local_without_raw_details() {
    let rendered = render_tui_script(["/details last", "/copy raw"], ".", ".");

    assert!(rendered.contains("No raw details are available."));
    assert!(!rendered.contains("> /details last"));
    assert!(!rendered.contains("> /copy raw"));
    assert!(!rendered.contains("stub provider response"));
}

#[test]
fn tui_script_clear_commands_clear_local_rendering_without_controller_call() {
    let root = std::env::temp_dir().join(format!("elgar-cli-clear-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();

    let rendered = render_tui_script(
        ["what does the harness do?", "/clear", "/new"],
        &root,
        &root,
    );

    assert!(!rendered.contains("> /clear"));
    assert!(!rendered.contains("> /new"));
    assert!(rendered.contains("stub provider response"));
    assert!(rendered.contains("(empty conversation)"));
    assert_eq!(rendered.matches("stub provider response").count(), 1);

    let _ = fs::remove_dir_all(root);
}

#[test]
fn tui_loop_reads_lines_and_exits_cleanly() {
    let input = b"what does the harness do?\n/quit\n";
    let mut output = Vec::new();

    run_tui_loop(&input[..], &mut output, ".", ".").unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert!(rendered.contains("Elgar TUI. Type /exit, /quit, or /q to leave."));
    assert!(rendered.contains("> what does the harness do?"));
    assert!(rendered.contains("Exiting Elgar TUI."));
}

#[test]
fn scripted_tui_renders_outside_path_pending_approval_warning() {
    let root =
        std::env::temp_dir().join(format!("elgar-cli-scripted-outside-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let provider = ToolCallProvider::write_file("/tmp/elgar-outside-warning-test.txt", "hello");
    let input = b"create outside file\n/exit\n";
    let mut output = Vec::new();

    run_tui_loop_with_runtime(&input[..], &mut output, &root, &root, provider).unwrap();

    let rendered = String::from_utf8(output).unwrap();
    assert!(rendered.contains("Pending approval"));
    assert!(rendered.contains("target: /tmp/elgar-outside-warning-test.txt"));
    assert!(rendered.contains("scope: outside_launch_folder"));
    assert!(rendered.contains("WARNING: Approving may modify files outside the launch folder."));

    let _ = fs::remove_dir_all(root);
}

#[derive(Clone)]
struct ToolCallProvider {
    outputs: Arc<Mutex<Vec<ProviderOutput>>>,
}

impl ToolCallProvider {
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
                ProviderOutput::new("write requires approval"),
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
