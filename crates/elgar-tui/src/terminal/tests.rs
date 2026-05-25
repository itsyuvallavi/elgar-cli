use std::{
    ffi::OsString,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, MutexGuard,
    },
};

use elgar_core::{
    action::{ActionLifecycleState, ActionRequest},
    context::{ContextAccounting, LoadedContextFile},
    controller::Controller,
    event::{
        AssistantMessage, AssistantMessageSource, Event, ProviderFinished, ProviderMetrics,
        ProviderOutput, ProviderStarted, ProviderTokenUsage,
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

fn submit_legacy_controller_input(
    shell: &mut TuiShell,
    controller: &Controller,
    session: &mut Session,
    input: &str,
) -> elgar_core::controller::TurnResult {
    let result = controller.turn(session, input);
    shell.consume_events(&result.events);
    result
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

mod commands_and_input;
mod copy_clipboard;
mod memory_commands;
mod model_first_live_flow;
mod rendering_frames;
mod startup_footer_layout;
