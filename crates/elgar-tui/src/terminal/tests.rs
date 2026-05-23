use std::{
    ffi::OsString,
    path::Path,
    sync::{Mutex, MutexGuard},
};

use elgar_core::{
    action::{ActionLifecycleState, ActionRequest},
    context::{ContextAccounting, LoadedContextFile},
    controller::Controller,
    event::{
        AssistantMessage, AssistantMessageSource, Event, ProviderMetrics, ProviderOutput,
        ProviderTokenUsage,
    },
    model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    provider::{
        ChatToolDefinition, ControllerProvider, ProviderError, ProviderRequestMetadata,
        ProviderStreamChunk,
    },
    session::Session,
};
use ratatui::{backend::TestBackend, Terminal};

use crate::{input::TerminalInput, panes::ConversationPane, TuiShell};

use super::prompt::{
    live_response_ansi, LIVE_REASONING_PREVIEW_BYTES, LIVE_RESPONSE_PREVIEW_BYTES,
};
use super::{
    active_working_frame_lines, conversation_print_blocks, copy_conversation_to_terminal_clipboard,
    copy_conversation_with_clipboards, default_shell_text, encode_base64, handle_scroll_key,
    handle_submitted_terminal_input_for_loop, handle_terminal_key,
    handle_terminal_key_with_copy_writer, inline_prompt_frame_lines, live_render_due,
    osc52_clipboard_sequence, parse_terminal_command, plain_block_lines, render_terminal_help,
    render_tui_shell, should_exit, status_style, style_terminal_conversation,
    transcript_output_ansi, LiveProviderOutput, ProviderTurnUpdate, TerminalCommand,
    TerminalShellContext, LIVE_RENDER_INTERVAL,
};

static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvGuard {
    name: &'static str,
    previous: Option<OsString>,
    _home_lock: Option<MutexGuard<'static, ()>>,
}

impl EnvGuard {
    fn set(name: &'static str, value: &Path) -> Self {
        let home_lock = (name == "HOME").then(|| {
            HOME_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        });
        let previous = std::env::var_os(name);
        std::env::set_var(name, value);
        Self {
            name,
            previous,
            _home_lock: home_lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var(self.name, previous);
        } else {
            std::env::remove_var(self.name);
        }
    }
}

fn draw_to_text(shell: &TuiShell, context: &TerminalShellContext) -> String {
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_tui_shell(frame, shell, context))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn terminal_user_message_renders_as_padded_block_without_prompt_marker() {
    let mut conversation = ConversationPane::default();
    conversation.push_pending_provider_turn("hello");
    let styled = style_terminal_conversation("startup", &conversation, 12);
    let user_line = styled
        .lines
        .iter()
        .find(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "hello       ")
        })
        .unwrap();

    assert_eq!(user_line.style, crate::theme::user_input_block());
}

#[test]
fn live_and_completed_provider_transcript_styles_match() {
    assert_eq!(live_response_ansi(), transcript_output_ansi());

    let mut conversation = ConversationPane::default();
    conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
        "completed response",
        AssistantMessageSource::Provider,
    )));
    let styled = style_terminal_conversation("startup", &conversation, 32);
    let completed_line = styled
        .lines
        .iter()
        .find(|line| {
            line.spans
                .first()
                .is_some_and(|span| span.content.as_ref() == "completed response")
        })
        .unwrap();

    assert_eq!(completed_line.style, crate::theme::model_output());
}

#[test]
fn terminal_markdown_code_blocks_print_with_compact_spacing() {
    let rendered = crate::markdown::render_assistant_markdown(
        "Use:\n\n```rust\n\nfn main() {}\n\n```\n\nDone.",
    );

    assert_eq!(
        plain_block_lines(&rendered, 80),
        vec!["Use:", "code (rust):", "    fn main() {}", "Done."]
    );
}

#[test]
fn inline_prompt_frame_matches_old_elgar_runtime_shape() {
    let context = TerminalShellContext::new("/Users/yuval/__git/elgar", "/Users/yuval/__git/elgar")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()));

    let (top, input, bottom, footer) = inline_prompt_frame_lines(&context, "hello", 86);

    assert_eq!(top.len(), 2);
    assert_eq!(top[0], "");
    assert!(top[1].starts_with("────"));
    assert_eq!(input, vec!["▸ hello▌"]);
    assert_eq!(bottom, vec![top[1].clone()]);
    assert_eq!(footer.len(), 1);
    assert!(footer[0].contains("openai/gpt-oss-20b"));
    assert!(!footer.join("\n").contains("select visible"));
    assert!(!footer.join("\n").contains("PgUp"));
    assert!(!footer.join("\n").contains("context:"));
}

#[test]
fn active_working_frame_keeps_prompt_and_footer_visible() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));

    let live_output = LiveProviderOutput::default();
    let (progress, reasoning, response, top, input, bottom, footer) =
        active_working_frame_lines(&context, 1, 7, "/cancel", &live_output, 80);

    assert_eq!(progress, vec!["", "Working with local model."]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
    assert_eq!(top[0], "");
    assert!(top[1].starts_with("────"));
    assert_eq!(input, vec!["▸ /cancel▌"]);
    assert_eq!(bottom, vec![top[1].clone()]);
    assert!(footer[0].contains("model-a"));
    assert_eq!(footer.len(), 1);
    assert!(!footer.join("\n").contains("context:"));
}

#[test]
fn inline_prompt_frame_shows_cursor_for_empty_input() {
    let context = TerminalShellContext::new("/repo", "/repo");

    let (_top, input, _bottom, _footer) = inline_prompt_frame_lines(&context, "", 80);

    assert_eq!(input, vec!["▸ ▌"]);
}

#[test]
fn active_working_frame_shows_initial_progress_before_provider_chunks() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let live_output = LiveProviderOutput::default();

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 0, "hello", &live_output, 80);

    assert_eq!(progress, vec!["", "Working with local model"]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_shows_live_reasoning_and_partial_response() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("Need greet.".to_string()));
    live_output.push_chunk(ProviderStreamChunk::Text("Hello".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert!(progress.is_empty());
    assert_eq!(reasoning, vec!["", "Greeting."]);
    assert_eq!(response, vec!["", "Hello"]);
    assert!(!reasoning.join("\n").contains("thinking"));
}

#[test]
fn active_working_frame_polishes_common_live_reasoning_prefixes() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to answer briefly.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_removes_instruction_filler_from_reasoning() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to respond as Elgar, short.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert!(reasoning.is_empty());
    assert!(!reasoning.join("\n").contains("short"));
    assert!(!reasoning.join("\n").contains("Elgar"));
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_hides_incomplete_need_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("Need".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 2, 1, "hello", &live_output, 80);

    assert_eq!(progress, vec!["", "Working with local model.."]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_hides_incomplete_we_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("We".to_string()));

    let (progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 3, 1, "hello", &live_output, 80);

    assert_eq!(progress, vec!["", "Working with local model..."]);
    assert!(reasoning.is_empty());
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_polishes_we_just_reasoning() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning("We just greet.".to_string()));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Greeting."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_polishes_we_need_live_reasoning_prefix() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "We need to inspect the prompt renderer tests.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Inspecting the prompt renderer tests."]);
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_does_not_turn_action_reasoning_into_action_claims() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "Need to write hello.py.".to_string(),
    ));

    let (_progress, reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 80);

    assert_eq!(reasoning, vec!["", "Write hello.py."]);
    assert!(!reasoning.join("\n").contains("Writing hello.py"));
    assert!(response.is_empty());
}

#[test]
fn active_working_frame_keeps_live_reasoning_summary_short() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(format!(
        "Need to explain {}",
        "local provider chunk handling ".repeat(20)
    )));

    let (_progress, reasoning, _response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "status", &live_output, 240);

    assert_eq!(reasoning.len(), 2);
    assert!(reasoning[1].chars().count() <= 160);
    assert!(reasoning[1].ends_with('…'));
}

#[test]
fn active_working_frame_reveals_response_before_completion() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "Hello! How can I help you today?".to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "hello", &live_output, 80);
    assert_eq!(response, vec!["", "Hello! How can I help you today?"]);
}

#[test]
fn active_working_frame_preserves_streamed_markdown_structure() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "I can:\n\n- Answer questions.\n- Summarise documents.".to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "what can you do?", &live_output, 80);

    assert_eq!(
        response,
        vec![
            "",
            "I can:",
            "- Answer questions.",
            "- Summarise documents."
        ]
    );
}

#[test]
fn live_and_completed_markdown_blocks_share_compact_spacing() {
    let response = "Sure! Let me suggest a small, clean folder structure.\n\ncode:\n\n    project/\n\n    src/\n\nWhat to do:\n\n1. Create directories.\n\n2. Move files.\n\nOnce you approve, I can generate commands.";
    let rendered = crate::markdown::render_assistant_markdown(response);
    let completed_lines = plain_block_lines(&rendered, 80);

    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(response.to_string()));

    let (_progress, _reasoning, live_lines, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "i meant create folders", &live_output, 80);

    assert_eq!(
        live_lines.into_iter().skip(1).collect::<Vec<_>>(),
        completed_lines
    );
    assert!(!rendered.contains("\n\n"));
}

#[test]
fn completed_terminal_transcript_groups_plain_lines_into_one_print_block() {
    let blocks = conversation_print_blocks(
        vec![
            (
                "Project Plan".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
            (
                "- First step.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
            (
                "- Second step.".to_string(),
                crate::panes::ConversationLineStyle::Plain,
            ),
        ],
        false,
    );

    assert_eq!(
        blocks,
        vec![(
            "Project Plan\n- First step.\n- Second step.".to_string(),
            crate::panes::ConversationLineStyle::Plain
        )]
    );
}

#[test]
fn active_working_frame_expands_inline_markdown_artifacts() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("model-a".to_string()));
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Text(
        "Use this: ```bash # 1. Start lm-studio --port 1234 # 2. Check curl http://127.0.0.1:1234/v1/health ``` Done."
            .to_string(),
    ));

    let (_progress, _reasoning, response, _top, _input, _bottom, _footer) =
        active_working_frame_lines(&context, 0, 1, "help", &live_output, 120);

    assert!(response.contains(&"Use this:".to_string()));
    assert!(response.contains(&"code (bash):".to_string()));
    assert!(response.contains(&"    # 1. Start lm-studio --port 1234".to_string()));
    assert!(response.contains(&"    # 2. Check curl http://127.0.0.1:1234/v1/health".to_string()));
    assert!(response.contains(&"Done.".to_string()));
}

#[test]
fn live_provider_output_caps_reasoning_and_response_preview_buffers() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_chunk(ProviderStreamChunk::Reasoning(
        "r".repeat(LIVE_REASONING_PREVIEW_BYTES + 512),
    ));
    live_output.push_chunk(ProviderStreamChunk::Text(
        "x".repeat(LIVE_RESPONSE_PREVIEW_BYTES + 512),
    ));

    assert!(live_output.reasoning_preview_bytes() <= LIVE_REASONING_PREVIEW_BYTES);
    assert!(live_output.response_preview_bytes() <= LIVE_RESPONSE_PREVIEW_BYTES);
}

#[test]
fn live_stream_redraws_are_throttled_to_ten_fps() {
    let last_render = std::time::Instant::now();

    assert!(!live_render_due(
        last_render,
        last_render + LIVE_RENDER_INTERVAL - std::time::Duration::from_millis(1)
    ));
    assert!(live_render_due(
        last_render,
        last_render + LIVE_RENDER_INTERVAL
    ));
}

fn submit_text(
    text: &str,
    input: &mut TerminalInput,
    controller: &Controller,
    session: &mut Session,
    shell: &mut TuiShell,
) -> bool {
    for character in text.chars() {
        let exited = handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            input,
            controller,
            session,
            shell,
        );
        assert!(!exited);
    }

    handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        input,
        controller,
        session,
        shell,
    )
}

fn wait_for_completed_provider_turn(
    task: &super::ProviderTurnTask,
) -> super::provider_task::CompletedProviderTurn {
    (0..20)
        .find_map(|_| {
            let result = task.try_complete().unwrap();
            match result {
                Some(ProviderTurnUpdate::Completed(completed)) => Some(*completed),
                Some(ProviderTurnUpdate::Chunk(_)) | None => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    None
                }
                Some(ProviderTurnUpdate::Canceled) => {
                    panic!("provider turn should complete, not cancel");
                }
            }
        })
        .expect("stub provider turn should complete")
}

fn finish_provider_turn(
    task: super::ProviderTurnTask,
    session: &mut Session,
    shell: &mut TuiShell,
) -> Vec<ProviderStreamChunk> {
    let mut chunks = Vec::new();
    let completed = (0..20)
        .find_map(|_| {
            let result = task.try_complete().unwrap();
            match result {
                Some(ProviderTurnUpdate::Chunk(chunk)) => {
                    chunks.push(chunk);
                    None
                }
                Some(ProviderTurnUpdate::Completed(completed)) => Some(completed),
                Some(ProviderTurnUpdate::Canceled) => panic!("provider turn should complete"),
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    None
                }
            }
        })
        .expect("provider turn should complete");

    *session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);
    chunks
}

#[test]
fn default_terminal_shell_is_empty_and_no_network() {
    let text = default_shell_text();

    assert!(text.contains("elgar v0.2"));
    assert!(text
        .contains("/commands · /clear · /cancel · /approve · /reject · /memory · /copy · /exit"));
    assert!(text
        .contains("Elgar uses your local LM Studio model and keeps file changes behind approval."));
    assert!(text.contains("[Context]"));
    assert!(text.contains("[Provider]\n  stub-provider · none"));
    assert!(text.contains("(empty conversation)"));
    assert!(text.contains("> "));
    assert!(!text.contains("context:"));
    let footer = TerminalShellContext::new(".", ".")
        .with_provider("stub-provider", None)
        .footer_body(
            "ready",
            "select visible text natively | PgUp/PgDn scroll | /copy conversation",
        );
    assert!(!footer.contains("select visible text natively"));
    assert!(!footer.contains("PgUp/PgDn"));
    assert!(!footer.contains("/copy conversation"));
    assert!(!footer.contains("repo:"));
    assert!(!footer.contains("cwd:"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains('|'));
    assert!(!text.contains("Ctrl+Y copy conversation"));
    assert!(!text.contains("br:"));
    assert!(text.contains("default no-network stub"));
    assert!(!text.contains("lm-studio"));
    assert!(!text.contains("Commands:"));
    assert!(!text.contains("Skills"));
    assert!(!text.contains("MCP"));
    assert!(!text.contains("Bash"));
    assert!(!text.contains("API"));
    assert!(!text.contains("settings"));
}

#[test]
fn terminal_startup_block_lists_real_context_files_and_configured_provider() {
    let root = temp_root("terminal-startup-context");
    std::fs::write(root.join("AGENTS.md"), "instructions").unwrap();
    std::fs::write(root.join("elgar-provider.json"), "{}").unwrap();
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(&root, &root)
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting::from_default_local_files(
            &root, &root, None,
        ));

    let text = draw_to_text(&shell, &context);

    assert!(text.contains("elgar v0.2"));
    assert!(text.contains("[Context]"));
    assert!(text.contains("AGENTS.md"));
    assert!(text.contains("elgar-provider.json"));
    assert!(text.contains("[Provider]"));
    assert!(text.contains("lm-studio · openai/gpt-oss-20b"));
    assert!(!text.contains("AGENTS.md, elgar-provider.json"));
    assert!(!text.contains("lm-studio / openai/gpt-oss-20b"));
    assert!(!text.contains("Commands:"));
    assert!(!text.contains("Skills"));
    assert!(!text.contains("MCP"));
    assert!(!text.contains("Bash"));
    assert!(!text.contains("API"));
    assert!(!text.contains("settings"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_layout_renders_default_shell_regions() {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new("/repo", "/repo/crates");
    let text = draw_to_text(&shell, &context);

    assert!(text.contains("(empty conversation)"));
    assert!(text.contains("> "));
    assert!(text.contains("repo crates"));
    assert!(!text.contains("context:"));
    assert!(!text.contains("br:"));
    assert!(!text.contains("select visible text"));
    assert!(!text.contains("provider:"));
    assert!(!text.contains("model:"));
    assert!(!text.contains("review action"));
    assert!(!text.contains("┌"));
    assert!(!text.contains("┐"));
    assert!(!text.contains("└"));
    assert!(!text.contains("┘"));
}

#[test]
fn terminal_layout_renders_pending_action_only_when_present() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "create file hello.py");
    shell.consume_events(&result.events);

    let text = draw_to_text(&shell, &TerminalShellContext::from_session(&session));

    assert!(text.contains("I can write hello.py. Approve to write it."));
    assert!(text.contains("review action"));
    assert!(text.contains("File: hello.py"));
    assert!(text.contains("Status: waiting for approval"));
    assert!(text.contains("No changes have been made yet"));
    assert!(text.contains("Use /approve to apply or /reject"));
    assert!(!text.contains("Action: action-1 CreateFile"));
    assert!(text.contains("> "));
    assert!(text.contains("review action"));
}

#[test]
fn terminal_footer_uses_provider_model_metadata_when_available() {
    let controller =
        Controller::new(elgar_core::provider::ProviderStub::new("local").with_model("model-a"));
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    let result = controller.turn(&mut session, "what does the harness do?");
    shell.consume_events(&result.events);

    let context = TerminalShellContext::from_session(&session);
    let text = draw_to_text(&shell, &context);
    let footer = context.footer_body("reply ready", "select visible text");

    assert!(text.contains("model-a"));
    assert!(footer.contains("model-a"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("reply ready"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("provider configured"));
    assert!(!footer.contains("stub/no-network"));
    assert!(!text.contains("Provider progress:"));
}

#[test]
fn terminal_footer_formats_repo_cwd_branch_model_and_context_placeholder() {
    let root = temp_root("terminal-footer-git-context");
    let cwd = root.join("crates").join("elgar-tui");
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        root.join(".git").join("HEAD"),
        "ref: refs/heads/feature/footer\n",
    )
    .unwrap();
    let context = TerminalShellContext::new(&root, &cwd)
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()));

    let footer = context.footer_body("ready", "select visible text");

    assert!(footer.contains(root.file_name().unwrap().to_str().unwrap()));
    assert!(footer.contains("crates/elgar-tui"));
    assert!(footer.contains("(feature/footer)"));
    assert!(footer.contains("openai/gpt-oss-20b"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("repo:"));
    assert!(!footer.contains("cwd:"));
    assert!(!footer.contains("branch:"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains('|'));
    assert!(!footer.contains('%'));
    assert!(!footer.contains("tokens"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_footer_hides_controller_context_accounting() {
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting {
            loaded_files: vec![LoadedContextFile {
                display_path: "AGENTS.md".to_string(),
                bytes: 1284,
                estimated_tokens: 321,
                truncated: false,
            }],
            omitted_files: Vec::new(),
            estimated_tokens: Some(321),
            max_window_tokens: Some(128_000),
        });

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(!footer.contains("~321"));
    assert!(!footer.contains("128k"));
    assert!(!footer.contains('%'));
    assert!(!footer.contains("TBD"));
}

#[test]
fn terminal_footer_hides_provider_usage_when_present() {
    let mut metrics = ProviderMetrics::new(
        "request-usage",
        Some("openai/gpt-oss-20b".to_string()),
        false,
        1,
        128,
    );
    metrics.usage = Some(ProviderTokenUsage {
        prompt_tokens: Some(7),
        completion_tokens: Some(3),
        total_tokens: Some(10),
    });
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()))
        .with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: Some(321),
            max_window_tokens: Some(128_000),
        })
        .with_provider_metrics(metrics);

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(!footer.contains("10/128k"));
    assert!(!footer.contains("~321"));
    assert!(!footer.contains('%'));
}

#[test]
fn terminal_context_from_session_carries_provider_usage_to_footer() {
    #[derive(Clone)]
    struct UsageProvider;

    impl ControllerProvider for UsageProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "usage-provider",
                Some("model-a".to_string()),
                "usage-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            let mut metrics =
                ProviderMetrics::new("usage-request-1", Some("model-a".to_string()), false, 1, 64);
            metrics.usage = Some(ProviderTokenUsage {
                prompt_tokens: Some(11),
                completion_tokens: Some(5),
                total_tokens: Some(16),
            });
            Ok(ProviderOutput::new("measured").with_metrics(metrics))
        }
    }

    let controller = Controller::new(UsageProvider);
    let mut session = Session::new("session-1", "/repo", "/repo");

    controller.turn(&mut session, "hello");

    let context = TerminalShellContext::from_session(&session);
    let footer = context.footer_body("ready", "copy");

    assert!(footer.contains("model-a"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("16 tokens"));
    assert!(!footer.contains('%'));
}

#[test]
fn terminal_footer_hides_context_when_provider_usage_is_absent() {
    let metrics = ProviderMetrics::new(
        "request-no-usage",
        Some("openai/gpt-oss-20b".to_string()),
        false,
        1,
        128,
    );
    let context = TerminalShellContext::new("/repo", "/repo")
        .with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: None,
        })
        .with_provider_metrics(metrics);

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(!footer.contains("TBD"));
    assert!(!footer.contains('%'));
}

#[test]
fn terminal_footer_hides_context_with_configured_window() {
    let context =
        TerminalShellContext::new("/repo", "/repo").with_context_accounting(ContextAccounting {
            loaded_files: Vec::new(),
            omitted_files: Vec::new(),
            estimated_tokens: None,
            max_window_tokens: Some(128_000),
        });

    let footer = context.footer_body("ready", "copy");

    assert!(!footer.contains("context:"));
    assert!(!footer.contains("128k"));
    assert!(!footer.contains('%'));
}

#[test]
fn terminal_loop_starts_provider_text_turn_as_active_pulse() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what does the harness do?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert_eq!(shell.status.render_body(), "◐ working");
    assert!(shell.status.provider_active());
    assert!(shell
        .conversation
        .render_body()
        .contains("> what does the harness do?\n◐ working"));
    shell.status.advance_thinking_pulse();
    shell.conversation.advance_loading_pulse();
    assert_eq!(shell.status.render_body(), "◓ working");
    assert!(shell.conversation.render_body().contains("◓ working"));

    let task = pending_turn.take().unwrap();
    let completed = wait_for_completed_provider_turn(&task);

    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert_eq!(session.events().len(), completed.events.len());
    assert_eq!(shell.status.render_body(), "reply ready");
    assert!(!shell.status.provider_active());
    assert!(!shell.render().contains("User\n"));
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Model:"));
}

#[test]
fn terminal_loop_sends_unclassified_non_slash_text_to_provider() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "sadsadad",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert!(shell
        .conversation
        .render_body()
        .contains("> sadsadad\n◐ working"));
    assert!(!shell
        .conversation
        .render_body()
        .contains("Input was not recognized"));

    let task = pending_turn.take().unwrap();
    let completed = wait_for_completed_provider_turn(&task);

    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert_eq!(session.events().len(), completed.events.len());
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Input was not recognized"));
}

#[test]
fn terminal_loop_keeps_prompt_marker_folder_plan_create_project_controller_owned() {
    let controller = Controller::default();
    let root = temp_root("terminal-folder-plan-execute-model-first");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "> create folder called helloworld",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(verified_folder.is_dir());
    assert_eq!(session.actions().len(), 1);
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(shell.render().contains("Created"));
    assert!(!shell.render().contains("Model-first tool call validated"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_model_first_same_folder_plan_uses_provider_tools_and_verified_folder() {
    let controller = Controller::default();
    let root = temp_root("terminal-same-folder-plan-model-first");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;
    let verified_folder = root.join("helloworld");

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called helloworld",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(verified_folder.is_dir());

    let provider_events_before_plan = provider_event_count(&session);
    assert!(!handle_submitted_terminal_input_for_loop(
        "create a plan for a simple React TS project in the same folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > provider_events_before_plan);

    let plan_path = verified_folder.join("react-ts-project-plan.md");
    let applied_plan = session
        .actions()
        .iter()
        .find(|record| {
            record.action.state == ActionLifecycleState::Applied
                && matches!(record.action.request, ActionRequest::CreateFile(_))
        })
        .expect("same-folder plan should be applied");
    let ActionRequest::CreateFile(action) = &applied_plan.action.request else {
        panic!("same-folder plan should create a Markdown file");
    };
    assert_eq!(
        action.target_path,
        std::path::PathBuf::from("helloworld/react-ts-project-plan.md")
    );
    assert!(applied_plan.verified_result.is_some());
    assert!(plan_path.is_file());
    assert!(!root.join("react-ts-project-plan.md").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_routes_unclassified_action_like_text_to_controller_not_provider() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create the local widget after setup",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Input was not recognized"));
}

#[test]
fn terminal_loop_polite_folder_request_uses_model_tool_path() {
    let controller = Controller::default();
    let root = temp_root("terminal-polished-folder-request");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create folder called review-guard",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(root.join("review-guard").is_dir());
    assert_eq!(session.actions().len(), 1);
    assert!(matches!(
        &session.actions()[0].action.request,
        ActionRequest::CreateDirectory(_)
    ));
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(!shell.render().contains("Input was not recognized"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_broad_react_project_request_uses_provider_not_controller_plan() {
    let controller = Controller::default();
    let root = temp_root("terminal-polished-react-project-request");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "can you please create a react project called demo",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(provider_event_count(&session) > 0);
    assert!(session.actions().is_empty());
    assert!(!root.join("demo").exists());
    assert!(!root.join("demo/react-project-plan.md").exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_loop_model_first_guidance_renders_naturally_without_creating_files() {
    let controller = Controller::default();
    let root = temp_root("terminal-model-first-guidance");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "create a project in that folder",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(chunks.is_empty());
    assert!(session.actions().is_empty());
    assert!(!root.join("project").exists());
    assert!(shell
        .render()
        .contains("Which folder should I use for the project?"));
    assert!(!shell.render().contains("Proposed action"));
    assert!(!shell.render().contains("Created"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_memory_command_is_local_and_empty_without_provider_call() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-empty");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_none());
    let rendered = shell.render();
    assert!(rendered.contains("Memory\n(empty)"));
    assert!(!rendered.contains("stub provider response"));
    assert!(!rendered.contains("lm-studio"));
    assert!(!rendered.contains("Input was not recognized"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_memory_command_reports_verified_project_state() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-project");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called src",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(!handle_submitted_terminal_input_for_loop(
        "create file project-plan.md",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(root.join("project-plan.md").is_file());
    assert!(pending_turn.is_none());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("Memory"));
    assert!(rendered.contains("folders\n- ok "));
    assert!(rendered.contains("plans\n- ok "));
    assert!(!rendered.contains("lm-studio"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_memory_command_reports_latest_provider_prompt_memory_trace() {
    let controller = Controller::default();
    let root = temp_root("terminal-memory-provider-trace");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "create folder called workspace",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(!handle_submitted_terminal_input_for_loop(
        "where did you put that folder?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    let completed = wait_for_completed_provider_turn(&pending_turn.take().unwrap());
    session = completed.session;
    shell.conversation.discard_pending_provider_turn();
    shell.consume_events(&completed.events);

    assert!(session.latest_provider_prompt_memory_selection().is_some());
    assert!(!handle_submitted_terminal_input_for_loop(
        "/memory",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let rendered = shell.render();
    assert!(rendered.contains("provider prompt memory"));
    assert!(rendered.contains("selected"));
    assert!(rendered.contains("verified folder ok "));
    assert!(!rendered.contains("Verified memory selected by Elgar controller:"));
    assert!(!rendered.contains("User request:"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn provider_turn_task_reports_canceled_without_applying_stale_result() {
    #[derive(Clone)]
    struct DelayedProvider;

    impl elgar_core::provider::ControllerProvider for DelayedProvider {
        fn request_metadata(&self) -> elgar_core::provider::ProviderRequestMetadata {
            elgar_core::provider::ProviderRequestMetadata::new(
                "delayed-provider",
                None,
                "delayed-request-1",
            )
        }

        fn chat(
            &self,
            _prompt: &str,
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
            Ok(elgar_core::event::ProviderOutput::new("late response"))
        }
    }

    let controller = Controller::new(DelayedProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::start_provider_turn(controller, session, "hello".to_string());

    task.cancel();
    std::thread::sleep(std::time::Duration::from_millis(30));

    assert!(matches!(
        task.try_complete().unwrap(),
        Some(ProviderTurnUpdate::Canceled)
    ));
}

#[test]
fn provider_turn_task_reports_streaming_chunks_before_completion() {
    #[derive(Clone)]
    struct StreamingProvider;

    impl elgar_core::provider::ControllerProvider for StreamingProvider {
        fn request_metadata(&self) -> elgar_core::provider::ProviderRequestMetadata {
            elgar_core::provider::ProviderRequestMetadata::new(
                "stream-provider",
                Some("model-a".to_string()),
                "stream-request-1",
            )
        }

        fn chat(
            &self,
            _prompt: &str,
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            Ok(elgar_core::event::ProviderOutput::new("Hello"))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<elgar_core::event::ProviderOutput, elgar_core::provider::ProviderError>
        {
            on_chunk(ProviderStreamChunk::Reasoning("Need greet.".to_string()));
            on_chunk(ProviderStreamChunk::Text("Hello".to_string()));
            Ok(elgar_core::event::ProviderOutput::new("Hello").with_thinking("Need greet."))
        }
    }

    let controller = Controller::new(StreamingProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::start_provider_turn(controller, session, "hello".to_string());
    let mut chunks = Vec::new();
    let completed = (0..20)
        .find_map(|_| {
            let result = task.try_complete().unwrap();
            match result {
                Some(ProviderTurnUpdate::Chunk(chunk)) => {
                    chunks.push(chunk);
                    None
                }
                Some(ProviderTurnUpdate::Completed(completed)) => Some(completed),
                Some(ProviderTurnUpdate::Canceled) => panic!("provider turn should complete"),
                None => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    None
                }
            }
        })
        .expect("stream provider turn should complete");

    assert_eq!(
        chunks,
        vec![
            ProviderStreamChunk::Reasoning("Need greet.".to_string()),
            ProviderStreamChunk::Text("Hello".to_string())
        ]
    );
    assert_eq!(completed.events.len(), 4);
}

#[test]
fn terminal_live_provider_dogfood_flow_keeps_provider_suggestions_and_actions_safe() {
    #[derive(Clone)]
    struct DogfoodProvider;

    impl ControllerProvider for DogfoodProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "dogfood-provider",
                Some("model-a".to_string()),
                "dogfood-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("Provider suggests creating hidden.py"))
        }

        fn chat_with_tools_with_metadata(
            &self,
            prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            if prompt.contains("create file approved.py") {
                return Ok(
                    ProviderOutput::new("Creating approved.py.").with_tool_calls(vec![
                        RawModelToolCall {
                            id: "dogfood-tool-call-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: serde_json::json!({
                                "target_path": "approved.py",
                                "contents": ""
                            }),
                            assistant_summary: Some("create approved.py".to_string()),
                        },
                    ]),
                );
            }

            if prompt.contains("create file rejected.py") {
                return Ok(
                    ProviderOutput::new("Creating rejected.py.").with_tool_calls(vec![
                        RawModelToolCall {
                            id: "dogfood-tool-call-1".to_string(),
                            name: RawModelToolName::Known(ModelToolName::CreateFile),
                            arguments: serde_json::json!({
                                "target_path": "rejected.py",
                                "contents": ""
                            }),
                            assistant_summary: Some("create rejected.py".to_string()),
                        },
                    ]),
                );
            }

            Ok(ProviderOutput::new("Provider suggests creating hidden.py")
                .with_thinking("Need answer without mutating files."))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<ProviderOutput, ProviderError> {
            on_chunk(ProviderStreamChunk::Reasoning(
                "Need answer without mutating files.".to_string(),
            ));
            on_chunk(ProviderStreamChunk::Text(
                "Provider suggests creating hidden.py".to_string(),
            ));
            Ok(ProviderOutput::new("Provider suggests creating hidden.py")
                .with_thinking("Need answer without mutating files."))
        }
    }

    let controller = Controller::new(DogfoodProvider);
    let root = temp_root("terminal-live-dogfood-flow");
    let hidden_target = root.join("hidden.py");
    let rejected_target = root.join("rejected.py");
    let approved_target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what should we create?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert!(shell.render().contains("◐ working"));

    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);

    assert!(chunks.is_empty());
    assert!(shell
        .render()
        .contains("Provider suggests creating hidden.py"));
    assert!(session.actions().is_empty());
    assert!(!hidden_target.exists());

    let mut input = TerminalInput::default();
    let mut output = Vec::new();
    for character in "/copy".chars() {
        let exited = handle_terminal_key_with_copy_writer(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
            &mut output,
        );
        assert!(!exited);
    }
    let exited = handle_terminal_key_with_copy_writer(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
        &mut output,
    );
    assert!(!exited);
    assert!(shell.copy.render_hint().starts_with("copied conversation"));
    assert!(!output.is_empty());

    assert!(!handle_submitted_terminal_input_for_loop(
        "/clear",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    assert!(shell.conversation.lines.is_empty());
    assert!(session.actions().is_empty());

    assert!(!handle_submitted_terminal_input_for_loop(
        "create file rejected.py",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(rejected_target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );

    assert!(!handle_submitted_terminal_input_for_loop(
        "create file approved.py",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));
    finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    assert!(approved_target.exists());
    assert_eq!(
        session.actions()[1].action.state,
        ActionLifecycleState::Applied
    );

    assert!(handle_submitted_terminal_input_for_loop(
        "/q",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_live_provider_dogfood_error_does_not_mutate_actions_or_files() {
    #[derive(Clone)]
    struct TimeoutProvider;

    impl ControllerProvider for TimeoutProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "timeout-provider",
                Some("model-a".to_string()),
                "timeout-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Err(ProviderError::network("provider request timed out"))
        }

        fn chat_with_tools_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
            _tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            Err(ProviderError::network("provider request timed out"))
        }
    }

    let controller = Controller::new(TimeoutProvider);
    let root = temp_root("terminal-live-dogfood-error");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    assert!(!handle_submitted_terminal_input_for_loop(
        "hello",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    ));

    let chunks = finish_provider_turn(pending_turn.take().unwrap(), &mut session, &mut shell);
    let rendered = shell.render();

    assert!(chunks.is_empty());
    assert!(rendered.contains("Provider error from timeout-provider"));
    assert!(rendered.contains("provider request timed out"));
    assert!(session.actions().is_empty());
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn completed_provider_turn_uses_final_output_not_capped_live_preview() {
    #[derive(Clone)]
    struct LargeStreamingProvider;

    impl ControllerProvider for LargeStreamingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "large-stream-provider",
                Some("model-a".to_string()),
                "large-stream-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("unused"))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(ProviderStreamChunk),
        ) -> Result<ProviderOutput, ProviderError> {
            let final_text = format!(
                "UNCAPPED_PREFIX_{}UNCAPPED_SUFFIX",
                "x".repeat(LIVE_RESPONSE_PREVIEW_BYTES + 512)
            );
            on_chunk(ProviderStreamChunk::Text(final_text.clone()));
            Ok(ProviderOutput::new(final_text))
        }
    }

    let controller = Controller::new(LargeStreamingProvider);
    let session = Session::new("session-1", "/repo", "/repo");
    let task = super::start_provider_turn(controller, session, "hello".to_string());
    let completed = wait_for_completed_provider_turn(&task);
    let mut shell = TuiShell::new();

    shell.consume_events(&completed.events);

    let rendered = shell.render();
    assert!(rendered.contains("UNCAPPED_PREFIX_"));
    assert!(rendered.contains("UNCAPPED_SUFFIX"));
}

#[test]
fn terminal_loop_cancel_drops_pending_provider_turn_without_session_mutation() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what does the harness do?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_some());
    assert!(shell.status.provider_active());

    let exited = handle_submitted_terminal_input_for_loop(
        "/cancel",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );

    assert!(!exited);
    assert!(pending_turn.is_none());
    assert!(session.events().is_empty());
    assert_eq!(shell.status.render_body(), "canceled");
    assert!(!shell.status.provider_active());
    assert!(shell.render().contains("Provider request canceled."));
    assert!(!shell.render().contains("stub provider response"));
}

#[test]
fn terminal_loop_cancel_drops_late_provider_completion_from_visible_and_session_path() {
    #[derive(Clone)]
    struct SlowProvider;

    impl ControllerProvider for SlowProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new("slow-provider", None, "slow-request-1")
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            std::thread::sleep(std::time::Duration::from_millis(30));
            Ok(ProviderOutput::new("late stale response"))
        }
    }

    let controller = Controller::new(SlowProvider);
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut pending_turn = None;

    let exited = handle_submitted_terminal_input_for_loop(
        "what does the harness do?",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );
    assert!(!exited);
    assert!(pending_turn.is_some());

    let exited = handle_submitted_terminal_input_for_loop(
        "/cancel",
        &controller,
        &mut session,
        &mut shell,
        &mut pending_turn,
    );
    std::thread::sleep(std::time::Duration::from_millis(60));

    assert!(!exited);
    assert!(pending_turn.is_none());
    assert!(session.events().is_empty());
    assert!(session.actions().is_empty());
    assert_eq!(shell.status.render_body(), "canceled");
    assert!(shell.render().contains("Provider request canceled."));
    assert!(!shell.render().contains("late stale response"));
    assert!(!shell.render().contains("slow-provider"));
}

#[test]
fn terminal_status_uses_named_theme_styles_by_state() {
    assert_eq!(status_style("ready"), crate::theme::success());
    assert_eq!(status_style("reply ready"), crate::theme::success());
    assert_eq!(status_style("◐ working"), crate::theme::thinking());
    assert_eq!(
        status_style("review action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(
        status_style("approved action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(
        status_style("rejected action-1"),
        crate::theme::warning_action()
    );
    assert_eq!(status_style("failed action-1"), crate::theme::error());
    assert_eq!(status_style("provider error"), crate::theme::error());
    assert_eq!(status_style("sent"), crate::theme::muted());
}

#[test]
fn terminal_footer_shows_lm_studio_provider_and_model_without_usage_claims() {
    let mut context = TerminalShellContext::new("/repo", "/repo");
    context.provider = Some("lm-studio".to_string());
    context.model = Some("openai/gpt-oss-20b".to_string());

    let footer = context.footer_body("ready", "select visible text");

    assert!(footer.contains("openai/gpt-oss-20b"));
    assert!(!footer.contains("context:"));
    assert!(!footer.contains("provider:"));
    assert!(!footer.contains("model:"));
    assert!(!footer.contains("select visible text"));
    assert!(!footer.contains("live/local"));
    assert!(!footer.contains("stub/no-network"));
}

#[test]
fn terminal_conversation_scrollback_keeps_input_status_and_pending_visible() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();

    shell.conversation.lines = (0..20).map(|index| format!("line {index}")).collect();
    let result = controller.turn(&mut session, "create file hello.py");
    shell.consume_events(&result.events);
    shell.conversation.scroll_up(100);

    let text = draw_to_text(&shell, &TerminalShellContext::from_session(&session));

    assert!(text.contains("elgar v0.2"));
    assert!(!text.contains("Review needed: action-1 CreateFile write hello.py"));
    assert!(text.contains("review action"));
    assert!(text.contains("File: hello.py"));
    assert!(!text.contains("Action: action-1 CreateFile"));
    assert!(text.contains("> "));
    assert!(text.contains("repo"));
}

#[test]
fn terminal_shell_exit_keys_are_minimal() {
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::NONE
    )));
    assert!(should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL
    )));
    assert!(should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL
    )));
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE
    )));
    assert!(!should_exit(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('q'),
        crossterm::event::KeyModifiers::NONE
    )));
}

#[test]
fn terminal_commands_are_slash_only() {
    assert_eq!(parse_terminal_command("/help"), TerminalCommand::Help);
    assert_eq!(parse_terminal_command(" /commands "), TerminalCommand::Help);
    assert_eq!(parse_terminal_command("/clear"), TerminalCommand::Clear);
    assert_eq!(parse_terminal_command(" /new "), TerminalCommand::Clear);
    assert_eq!(parse_terminal_command("/approve"), TerminalCommand::Approve);
    assert_eq!(parse_terminal_command("/reject"), TerminalCommand::Reject);
    assert_eq!(parse_terminal_command("/cancel"), TerminalCommand::Cancel);
    assert_eq!(parse_terminal_command("/memory"), TerminalCommand::Memory);
    assert_eq!(parse_terminal_command("/copy"), TerminalCommand::Copy);
    assert_eq!(parse_terminal_command("/exit"), TerminalCommand::Exit);
    assert_eq!(parse_terminal_command("/quit"), TerminalCommand::Exit);
    assert_eq!(parse_terminal_command("/q"), TerminalCommand::Exit);
    assert_eq!(
        parse_terminal_command("/model"),
        TerminalCommand::Unknown("/model")
    );
    assert_eq!(
        parse_terminal_command("clear"),
        TerminalCommand::Text("clear")
    );
    assert_eq!(parse_terminal_command("new"), TerminalCommand::Text("new"));
    assert_eq!(parse_terminal_command("q"), TerminalCommand::Text("q"));
    assert_eq!(
        parse_terminal_command("quit"),
        TerminalCommand::Text("quit")
    );
    assert_eq!(
        parse_terminal_command("approve"),
        TerminalCommand::Text("approve")
    );
    assert_eq!(
        parse_terminal_command("reject"),
        TerminalCommand::Text("reject")
    );

    let help = render_terminal_help();
    assert!(help.starts_with("Commands\n/commands"));
    assert!(help.contains("/clear"));
    assert!(help.contains("/new"));
    assert!(help.contains("/approve"));
    assert!(help.contains("/reject"));
    assert!(help.contains("/cancel"));
    assert!(help.contains("/memory"));
    assert!(help.contains("/copy"));
    assert!(help.contains("/exit"));
    assert!(help.contains("/quit"));
    assert!(help.contains("/q"));
    assert!(help.contains("/help"));
    assert!(!help.contains("/model"));
    assert!(!help.contains("/settings"));
    assert!(!help.contains("/bash"));
    assert!(!help.contains("/api"));
}

#[test]
fn terminal_plain_approval_words_do_not_apply_pending_actions() {
    let controller = Controller::default();
    let root = temp_root("terminal-plain-approval-blocked");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    shell.submit_input(&controller, &mut session, "create file approved.py");
    let before_session = session.clone();

    let exited = submit_text("approve", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert!(!target.exists());
    assert_eq!(session, before_session);
    assert!(shell
        .render()
        .contains("Action commands must use /approve or /reject."));
    assert!(input.text().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_clear_slash_commands_clear_only_local_conversation() {
    let controller = Controller::default();
    let root = temp_root("terminal-clear-local");
    let target = root.join("clear.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    shell.submit_input(&controller, &mut session, "create file clear.py");
    let before_session = session.clone();
    let before_pending = shell.pending_action.clone();

    let exited = submit_text("/clear", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(shell.pending_action, before_pending);
    assert!(shell.conversation.lines.is_empty());
    assert!(!target.exists());
    assert!(input.text().is_empty());

    shell.conversation.lines.push("visible again".to_string());
    let exited = submit_text("/new", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(shell.pending_action, before_pending);
    assert!(shell.conversation.lines.is_empty());
    assert!(!target.exists());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_provider_active_enter_preserves_non_cancel_draft() {
    let mut input = TerminalInput::default();

    for character in "keep this draft".chars() {
        assert_eq!(
            super::handle_active_provider_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut input,
            ),
            super::ActiveProviderKeyAction::Continue
        );
    }

    assert_eq!(input.text(), "keep this draft");
    assert_eq!(
        super::handle_active_provider_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
        ),
        super::ActiveProviderKeyAction::Continue
    );
    assert_eq!(input.text(), "keep this draft");
}

#[test]
fn terminal_provider_active_enter_consumes_cancel_command() {
    let mut input = TerminalInput::default();

    for character in "/cancel".chars() {
        assert_eq!(
            super::handle_active_provider_key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut input,
            ),
            super::ActiveProviderKeyAction::Continue
        );
    }

    assert_eq!(
        super::handle_active_provider_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
        ),
        super::ActiveProviderKeyAction::Cancel
    );
    assert_eq!(input.text(), "");
}

#[test]
fn terminal_page_keys_update_only_ui_scrollback() {
    let session = Session::new("session-1", "/repo", "/repo");
    let before_session = session.clone();
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    let before_lines = shell.conversation.lines.clone();

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut shell,
    ));
    assert_eq!(shell.conversation.scroll_offset(4), 1);

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::PageDown,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut shell,
    ));
    assert_eq!(shell.conversation.scroll_offset(4), 6);

    assert_eq!(session, before_session);
    assert_eq!(shell.conversation.lines, before_lines);
    assert!(session.events().is_empty());
}

#[test]
fn terminal_copy_prefers_system_clipboard_without_terminal_escape() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = vec![
        "first visible line".to_string(),
        "older scrolled line".to_string(),
    ];
    let mut output = Vec::new();

    copy_conversation_with_clipboards(&mut output, &mut shell, |text| {
        assert_eq!(text, "first visible line\nolder scrolled line");
        Ok(())
    })
    .unwrap();

    assert!(output.is_empty());
    assert_eq!(shell.copy.render_hint(), "copied conversation (38 bytes)");
}

#[test]
fn terminal_copy_uses_osc52_for_full_rendered_conversation() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = vec![
        "first visible line".to_string(),
        "older scrolled line".to_string(),
    ];
    let mut output = Vec::new();

    copy_conversation_to_terminal_clipboard(&mut output, &mut shell).unwrap();

    let copied = String::from_utf8(output).unwrap();
    assert_eq!(
        copied,
        osc52_clipboard_sequence("first visible line\nolder scrolled line")
    );
    assert_eq!(shell.copy.render_hint(), "copied conversation (38 bytes)");
}

#[test]
fn terminal_copy_reports_failure_when_system_and_terminal_clipboards_fail() {
    struct FailingWriter;

    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("terminal rejected OSC 52"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut shell = TuiShell::new();
    shell.conversation.lines = vec!["copy target".to_string()];

    let error = copy_conversation_with_clipboards(FailingWriter, &mut shell, |_text| {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "pbcopy missing",
        ))
    })
    .unwrap_err();

    assert!(error.to_string().contains("pbcopy missing"));
    assert!(error.to_string().contains("terminal rejected OSC 52"));
    assert!(shell
        .copy
        .render_hint()
        .contains("system clipboard failed: pbcopy missing"));
}

#[test]
fn terminal_copy_slash_command_does_not_change_controller_or_scroll_state() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let before_session = session.clone();
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);
    let mut input = TerminalInput::default();

    let mut output = Vec::new();
    for character in "/copy".chars() {
        let exited = handle_terminal_key_with_copy_writer(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
            &mut output,
        );
        assert!(!exited);
    }

    let exited = handle_terminal_key_with_copy_writer(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
        &mut output,
    );

    assert!(!exited);
    assert_eq!(session, before_session);
    assert_eq!(input.text(), "");
    assert_eq!(shell.conversation.scroll_offset(4), 1);
    assert!(shell.copy.render_hint().starts_with("copied conversation"));
    assert_eq!(
        String::from_utf8(output).unwrap(),
        osc52_clipboard_sequence(&shell.conversation_copy_text())
    );
}

#[test]
fn terminal_clipboard_encoding_is_standard_base64() {
    assert_eq!(encode_base64(b""), "");
    assert_eq!(encode_base64(b"f"), "Zg==");
    assert_eq!(encode_base64(b"fo"), "Zm8=");
    assert_eq!(encode_base64(b"foo"), "Zm9v");
    assert_eq!(
        osc52_clipboard_sequence("copy me"),
        "\x1b]52;c;Y29weSBtZQ==\x07"
    );
}

#[test]
fn terminal_plain_end_edits_input_instead_of_following_latest() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);
    let mut input = TerminalInput::default();

    for code in [
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyCode::Left,
        crossterm::event::KeyCode::End,
        crossterm::event::KeyCode::Char('d'),
    ] {
        handle_terminal_key(
            crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );
    }

    assert_eq!(input.text(), "acd");
    assert_eq!(shell.conversation.scroll_offset(4), 1);
}

#[test]
fn terminal_ctrl_end_follows_latest() {
    let mut shell = TuiShell::new();
    shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
    shell.conversation.scroll_up(5);

    assert!(handle_scroll_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::End,
            crossterm::event::KeyModifiers::CONTROL,
        ),
        &mut shell,
    ));

    assert_eq!(shell.conversation.scroll_offset(4), 6);
}

#[test]
fn terminal_enter_submits_input_through_controller_backed_shell() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    for character in "what does the harness do?".chars() {
        let exited = handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(character),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );
        assert!(!exited);
    }

    let exited = handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(input.text().is_empty());
    assert!(shell.render().contains("> what does the harness do?"));
    assert!(!shell.render().contains("User\n"));
    assert!(shell.render().contains("stub provider response"));
    assert!(!shell.render().contains("Model:"));
    assert_eq!(session.events().len(), 4);
}

#[test]
fn terminal_greeting_uses_stub_chat_with_live_path_guidance() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    let exited = submit_text("hello!", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    let rendered = shell.render();
    assert!(rendered.contains("> hello!"));
    assert!(!rendered.contains("User\n"));
    assert!(!rendered.contains("Model:"));
    assert!(rendered.contains("stub provider response (no-network) to: hello!"));
    assert!(rendered.contains("No live provider call was made"));
    assert!(rendered.contains("tui-controller-smoke"));
    assert!(!rendered.contains("Input was not recognized"));
    assert!(session.actions().is_empty());
}

#[test]
fn terminal_enter_ignores_empty_input_without_controller_turn() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(' '),
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );
    let exited = handle_terminal_key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(session.events().is_empty());
    assert!(input.text().is_empty());
}

#[test]
fn terminal_approve_slash_command_approves_pending_action_through_shell() {
    let controller = Controller::default();
    let root = temp_root("terminal-slash-approve");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    shell.submit_input(&controller, &mut session, "create file approved.py");
    assert!(!target.exists());

    let exited = submit_text(
        "/approve",
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );

    assert!(!exited);
    assert!(target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Applied
    );
    assert!(shell.render().contains("Status: applied and verified"));
    assert!(shell.render().contains("Result: Wrote"));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_reject_slash_command_rejects_pending_action_through_shell() {
    let controller = Controller::default();
    let root = temp_root("terminal-slash-reject");
    let target = root.join("rejected.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    shell.submit_input(&controller, &mut session, "create file rejected.py");

    let exited = submit_text("/reject", &mut input, &controller, &mut session, &mut shell);

    assert!(!exited);
    assert!(!target.exists());
    assert_eq!(
        session.actions()[0].action.state,
        ActionLifecycleState::Rejected
    );
    assert!(shell.render().contains("Status: rejected"));
    assert!(shell
        .render()
        .contains("Result: Rejected. No file was changed."));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn terminal_approval_slash_commands_show_no_pending_feedback() {
    let controller = Controller::default();
    let mut session = Session::new("session-1", "/repo", "/repo");
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    submit_text(
        "/approve",
        &mut input,
        &controller,
        &mut session,
        &mut shell,
    );
    submit_text("/reject", &mut input, &controller, &mut session, &mut shell);

    let rendered = shell.render();
    assert!(rendered.contains("No proposed action is waiting for approval."));
    assert!(rendered.contains("No proposed action is waiting for rejection."));
    assert!(input.text().is_empty());
    assert!(session.actions().is_empty());
}

#[test]
fn terminal_function_keys_and_ctrl_y_are_not_command_actions() {
    let controller = Controller::default();
    let root = temp_root("terminal-no-key-commands");
    let target = root.join("approved.py");
    let mut session = Session::new("session-1", root.clone(), root.clone());
    let mut shell = TuiShell::new();
    let mut input = TerminalInput::default();

    shell.submit_input(&controller, &mut session, "create file approved.py");
    let before_session = session.clone();

    for key in [
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(5),
            crossterm::event::KeyModifiers::NONE,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(6),
            crossterm::event::KeyModifiers::NONE,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
            crossterm::event::KeyModifiers::NONE,
        ),
    ] {
        let exited = handle_terminal_key(key, &mut input, &controller, &mut session, &mut shell);
        assert!(!exited);
    }

    assert!(!target.exists());
    assert_eq!(session, before_session);
    assert_eq!(input.text(), "q");
    assert_eq!(shell.copy.render_hint(), "");

    let _ = std::fs::remove_dir_all(root);
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("elgar-terminal-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn provider_event_count(session: &Session) -> usize {
    session
        .events()
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::ProviderStarted(_) | Event::ProviderFinished(_)
            )
        })
        .count()
}
