use std::{
    io::{self, Write},
    time::{Duration, Instant},
};

use crossterm::{
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyEventKind,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode},
};
use elgar_core::{
    action_gate::ActionGate, agent_runtime::AgentRuntime, provider::ControllerProvider,
    session::Session,
};

use crate::{
    input::{TerminalInput, TerminalInputAction},
    memory::{
        render_session_created_actions, render_session_memory, render_session_pending_action,
        render_session_plan_preview, render_session_state_snapshot, render_session_status,
    },
    terminal::{
        commands::{
            clear_terminal_conversation, clear_visible_terminal,
            copy_conversation_to_terminal_clipboard, parse_terminal_command, render_terminal_help,
            TerminalCommand,
        },
        context::{terminal_context, TerminalShellContext},
        keymap::{
            handle_submitted_terminal_input, handle_terminal_text_input,
            normalize_terminal_provider_text_input, terminal_text_should_run_inline_provider_text,
        },
        prompt::{
            frame_separator_line, terminal_width, InlinePromptRenderer, InlineWorkingRenderer,
            LiveProviderOutput,
        },
        provider_task::{
            start_provider_text_turn, start_tool_turn, ProviderTurnTask, ProviderTurnUpdate,
        },
        render::{
            print_and_record_local, print_new_conversation_lines, print_plain_block, print_spacer,
            print_user_block, render_terminal_startup,
        },
    },
    turn_metrics::{aggregate_provider_token_usage, duration_millis},
    TuiShell,
};

use super::{
    ANSI_BOLD, ANSI_CURSOR_HIDE, ANSI_CURSOR_SHOW, ANSI_CYAN, ANSI_MUTED, ANSI_RESET,
    IDLE_RENDER_INTERVAL,
};

#[cfg(test)]
pub(crate) const LIVE_RENDER_INTERVAL: Duration = Duration::from_millis(100);

pub(super) fn print_inline_startup(context: &TerminalShellContext) -> io::Result<()> {
    writeln!(io::stdout())?;
    writeln!(
        io::stdout(),
        "{ANSI_MUTED}{}{ANSI_RESET}",
        frame_separator_line(terminal_width())
    )?;
    for line in render_terminal_startup(context).lines() {
        if line.starts_with("elgar") || line.starts_with('[') {
            writeln!(io::stdout(), "{ANSI_CYAN}{ANSI_BOLD}{line}{ANSI_RESET}")?;
        } else if line.trim().is_empty() {
            writeln!(io::stdout())?;
        } else {
            writeln!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}")?;
        }
    }
    writeln!(io::stdout())?;
    io::stdout().flush()
}

pub(super) fn read_inline_prompt(
    context: &TerminalShellContext,
    initial_input: &str,
) -> io::Result<Option<String>> {
    let _guard = TerminalModeGuard::enter()?;
    let mut input = TerminalInput::from_text(initial_input);
    let mut renderer = InlinePromptRenderer::new(context.clone());
    renderer.render(input.text())?;

    loop {
        match handle_terminal_input_event(event::read()?, &mut input) {
            TerminalInputAction::Continue => renderer.render(input.text())?,
            TerminalInputAction::Submit => {
                let submitted = input.drain().trim().to_string();
                renderer.clear()?;
                return Ok(Some(submitted));
            }
            TerminalInputAction::Exit => {
                renderer.clear()?;
                return Ok(None);
            }
        }
    }
}

pub(crate) fn handle_inline_submission<P>(
    submitted: &str,
    runtime: &AgentRuntime<P>,
    action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<(bool, String)>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Empty => Ok((false, String::new())),
        TerminalCommand::Exit => Ok((true, String::new())),
        TerminalCommand::Help => {
            print_and_record_local(shell, render_terminal_help())?;
            Ok((false, String::new()))
        }
        TerminalCommand::Clear => {
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
        TerminalCommand::Cancel => {
            print_and_record_local(shell, "No provider request is running.")?;
            Ok((false, String::new()))
        }
        TerminalCommand::Memory => {
            print_and_record_local(shell, render_session_memory(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::State => {
            print_and_record_local(shell, render_session_state_snapshot(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::PlanPreview => {
            print_and_record_local(shell, render_session_plan_preview(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Reasoning => {
            print_and_record_local(shell, crate::render_session_reasoning(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Status => {
            print_and_record_local(shell, render_session_status(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Tokens => {
            print_and_record_local(shell, crate::render_session_tokens(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Pending => {
            print_and_record_local(shell, render_session_pending_action(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Created => {
            print_and_record_local(shell, render_session_created_actions(session))?;
            Ok((false, String::new()))
        }
        TerminalCommand::Permissions(argument) => {
            let message = shell.apply_permission_command(argument);
            print_and_record_local(shell, message)?;
            Ok((false, String::new()))
        }
        TerminalCommand::Tool(text) => {
            let preserved_input = run_inline_tool_turn(text, runtime, session, shell)?;
            Ok((false, preserved_input))
        }
        TerminalCommand::Unknown(command) => {
            print_and_record_local(
                shell,
                format!("Unknown command: {command}. Type /commands for commands."),
            )?;
            Ok((false, String::new()))
        }
        TerminalCommand::Approve | TerminalCommand::Reject => {
            let before = shell.conversation.render_lines_with_styles().len();
            let exit = handle_submitted_terminal_input(
                submitted,
                runtime,
                action_gate,
                session,
                shell,
                io::stdout(),
            );
            print_new_conversation_lines(shell, before, false, false)?;
            Ok((exit, String::new()))
        }
        TerminalCommand::Text(text) => {
            if terminal_text_should_run_inline_provider_text(text) {
                let provider_input = normalize_terminal_provider_text_input(text);
                let preserved_input =
                    run_inline_provider_text_turn(&provider_input, runtime, session, shell)?;
                Ok((false, preserved_input))
            } else {
                let before = shell.conversation.render_lines_with_styles().len();
                handle_terminal_text_input(text, runtime, action_gate, session, shell);
                print_new_conversation_lines(shell, before, false, false)?;
                Ok((false, String::new()))
            }
        }
    }
}

fn run_inline_tool_turn<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<String>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    run_inline_provider_turn(text, runtime, session, shell, true)
}

fn run_inline_provider_text_turn<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<String>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    run_inline_provider_turn(text, runtime, session, shell, false)
}

fn run_inline_provider_turn<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    tool_enabled: bool,
) -> io::Result<String>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let turn_started = Instant::now();
    let before = shell.conversation.render_lines_with_styles().len();
    print_spacer()?;
    let visible_input = if tool_enabled {
        format!("/tool {text}")
    } else {
        text.to_string()
    };
    print_user_block(&visible_input)?;

    let task = if tool_enabled {
        start_tool_turn(
            runtime.clone(),
            session.clone(),
            text.to_string(),
            shell.policy_mode,
        )
    } else {
        start_provider_text_turn(
            runtime.clone(),
            session.clone(),
            text.to_string(),
            shell.policy_mode,
        )
    };
    let guard = TerminalModeGuard::enter()?;
    let mut working =
        InlineWorkingRenderer::new(terminal_context(session, runtime, shell.policy_mode));
    let mut input = TerminalInput::default();
    let mut live_output = LiveProviderOutput::default();
    live_output.suppress_reasoning_preview();
    live_output.suppress_response_preview();
    let mut tick = 0usize;
    working.render(
        tick,
        turn_started.elapsed().as_secs(),
        input.text(),
        &live_output,
    )?;
    tick = tick.wrapping_add(1);
    let mut last_render = Instant::now();

    let completed = loop {
        match task.try_complete() {
            #[cfg(test)]
            Ok(Some(ProviderTurnUpdate::Chunk(chunk))) => {
                live_output.push_chunk(chunk);
            }
            Ok(Some(ProviderTurnUpdate::Completed(completed))) => break completed,
            Ok(Some(ProviderTurnUpdate::Canceled)) => {
                working.clear()?;
                drop(guard);
                print_plain_block("Provider request canceled.")?;
                return Ok(String::new());
            }
            Ok(None) => {
                if last_render.elapsed() >= IDLE_RENDER_INTERVAL {
                    working.render(
                        tick,
                        turn_started.elapsed().as_secs(),
                        input.text(),
                        &live_output,
                    )?;
                    tick = tick.wrapping_add(1);
                    last_render = Instant::now();
                }

                if event::poll(Duration::from_millis(60))? {
                    handle_active_provider_event(
                        &task,
                        &mut input,
                        &mut working,
                        tick,
                        turn_started.elapsed().as_secs(),
                        &live_output,
                    )?;
                }
            }
            Err(message) => {
                working.clear()?;
                drop(guard);
                print_plain_block(&format!("Provider error: {message}"))?;
                return Ok(String::new());
            }
        }
    };

    let preserved_input = input.text().to_string();
    working.clear()?;
    drop(guard);
    let completed = *completed;
    let turn_duration_millis = duration_millis(turn_started.elapsed());
    let turn_usage = aggregate_provider_token_usage(&completed.events);
    *session = completed.session;
    shell.consume_events(&completed.events);
    shell
        .conversation
        .push_turn_metrics(turn_duration_millis, turn_usage.as_ref());
    shell.conversation.follow_latest();
    print_new_conversation_lines(shell, before, true, true)?;
    Ok(preserved_input)
}

#[cfg(test)]
pub(crate) fn live_render_due(last_render: Instant, now: Instant) -> bool {
    now.duration_since(last_render) >= LIVE_RENDER_INTERVAL
}

fn handle_active_provider_event(
    task: &ProviderTurnTask,
    input: &mut TerminalInput,
    working: &mut InlineWorkingRenderer,
    tick: usize,
    elapsed_secs: u64,
    live_output: &LiveProviderOutput,
) -> io::Result<()> {
    match handle_active_provider_input_event(event::read()?, input) {
        ActiveProviderKeyAction::Continue => {
            working.render(tick, elapsed_secs, input.text(), live_output)
        }
        ActiveProviderKeyAction::Cancel => {
            task.cancel();
            working.render(tick, elapsed_secs, input.text(), live_output)
        }
        ActiveProviderKeyAction::Exit => {
            task.cancel();
            Ok(())
        }
    }
}

pub(crate) fn handle_terminal_input_event(
    event: crossterm::event::Event,
    input: &mut TerminalInput,
) -> TerminalInputAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => input.handle_key(key),
        Event::Paste(text) => {
            input.insert_text(&text);
            TerminalInputAction::Continue
        }
        _ => TerminalInputAction::Continue,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveProviderKeyAction {
    Continue,
    Cancel,
    Exit,
}

pub(crate) fn handle_active_provider_key(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
) -> ActiveProviderKeyAction {
    match input.handle_key(key) {
        TerminalInputAction::Continue => ActiveProviderKeyAction::Continue,
        TerminalInputAction::Submit => {
            if matches!(
                parse_terminal_command(input.text()),
                TerminalCommand::Cancel
            ) {
                let _ = input.drain();
                ActiveProviderKeyAction::Cancel
            } else {
                ActiveProviderKeyAction::Continue
            }
        }
        TerminalInputAction::Exit => ActiveProviderKeyAction::Exit,
    }
}

pub(crate) fn handle_active_provider_input_event(
    event: crossterm::event::Event,
    input: &mut TerminalInput,
) -> ActiveProviderKeyAction {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            handle_active_provider_key(key, input)
        }
        Event::Paste(text) => {
            input.insert_text(&text);
            ActiveProviderKeyAction::Continue
        }
        _ => ActiveProviderKeyAction::Continue,
    }
}

struct TerminalModeGuard;

impl TerminalModeGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(
            stdout,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        write!(stdout, "{ANSI_CURSOR_HIDE}")?;
        stdout.flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        let _ = execute!(stdout, PopKeyboardEnhancementFlags, DisableBracketedPaste);
        let _ = write!(stdout, "{ANSI_CURSOR_SHOW}");
        let _ = stdout.flush();
        let _ = disable_raw_mode();
    }
}
