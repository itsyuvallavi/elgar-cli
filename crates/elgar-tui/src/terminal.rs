use std::{
    io::{self, Write},
    path::{Path, PathBuf},
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
    action_gate::ActionGate,
    agent_runtime::AgentRuntime,
    context::ContextAccounting,
    event::ProviderMetrics,
    policy::PermissionPolicyMode,
    provider::{ControllerProvider, ProviderConfig},
    router::normalize_pasted_transcript_input,
    session::Session,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

#[cfg(test)]
use elgar_core::controller::Controller;

use crate::{
    input::{TerminalInput, TerminalInputAction},
    memory::{
        render_session_created_actions, render_session_memory, render_session_pending_action,
        render_session_plan_preview, render_session_state_snapshot, render_session_status,
    },
    panes::{ConversationLineStyle, ConversationPane},
    startup::StartupBlock,
    theme, TuiShell,
};

mod commands;
mod footer;
mod prompt;
mod provider_task;
mod text;

use commands::{
    clear_terminal_conversation, clear_visible_terminal, copy_conversation_to_terminal_clipboard,
    parse_terminal_command, render_terminal_help, TerminalCommand,
};
#[cfg(test)]
use commands::{copy_conversation_with_clipboards, encode_base64, osc52_clipboard_sequence};
use footer::{align_footer_line, footer_location_label};
#[cfg(test)]
use prompt::{active_working_frame_lines, inline_prompt_frame_lines};
use prompt::{
    drawable_width, frame_separator_line, non_empty_lines, terminal_width, wrap_words,
    InlinePromptRenderer, InlineWorkingRenderer, LiveProviderOutput,
};
#[cfg(test)]
use provider_task::start_provider_turn;
use provider_task::{
    start_provider_text_turn, start_tool_turn, ProviderTurnTask, ProviderTurnUpdate,
};
use text::{conversation_print_blocks, pad_line, plain_block_lines};

#[cfg(test)]
const LIVE_RENDER_INTERVAL: Duration = Duration::from_millis(100);
const IDLE_RENDER_INTERVAL: Duration = Duration::from_millis(140);

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_CYAN: &str = "\x1b[38;2;143;207;198m";
const ANSI_MUTED: &str = "\x1b[38;2;118;126;126m";
const ANSI_TEXT: &str = "\x1b[38;2;214;219;224m";
const ANSI_TOOL_BLOCK: &str = "\x1b[38;2;186;214;194m\x1b[48;2;29;45;34m";
const ANSI_USER_BLOCK: &str = "\x1b[1m\x1b[38;2;143;207;198m\x1b[48;2;8;32;32m";
const ANSI_CURSOR_HIDE: &str = "\x1b[?25l";
const ANSI_CURSOR_SHOW: &str = "\x1b[?25h";

fn transcript_output_ansi() -> &'static str {
    ANSI_TEXT
}

pub fn run_terminal_shell() -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_at(&cwd, &cwd)
}

pub fn run_terminal_shell_with_lm_studio_provider(config: ProviderConfig) -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let context_window_tokens = config.configured_context_window_tokens();
    run_terminal_shell_with_runtime(
        &cwd,
        &cwd,
        AgentRuntime::with_lm_studio_provider(config),
        context_window_tokens,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_at(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_with_lm_studio_provider_at_with_policy(
        config,
        project_root,
        cwd,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_with_lm_studio_provider_at_with_policy(
    config: ProviderConfig,
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()> {
    let context_window_tokens = config.configured_context_window_tokens();
    run_terminal_shell_with_runtime(
        project_root,
        cwd,
        AgentRuntime::with_lm_studio_provider(config),
        context_window_tokens,
        policy_mode,
    )
}

pub fn run_terminal_shell_at(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_at_with_policy(
        project_root,
        cwd,
        PermissionPolicyMode::AutoCreateReviewModify,
    )
}

pub fn run_terminal_shell_at_with_policy(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()> {
    run_terminal_shell_with_runtime(
        project_root,
        cwd,
        AgentRuntime::default(),
        None,
        policy_mode,
    )
}

fn run_terminal_shell_with_runtime<P>(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    runtime: AgentRuntime<P>,
    context_window_tokens: Option<u64>,
    policy_mode: PermissionPolicyMode,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let mut session = Session::new("terminal-tui-session", project_root.as_ref(), cwd.as_ref());
    runtime.refresh_context_accounting(&mut session, context_window_tokens);
    let action_gate = ActionGate::new(runtime.provider.clone());
    let mut shell = TuiShell::with_policy_mode(policy_mode);

    let mut context = terminal_context(&session, &runtime, shell.policy_mode);
    print_inline_startup(&context)?;

    let mut next_prompt_input = String::new();
    loop {
        runtime.refresh_context_accounting(&mut session, context_window_tokens);
        context = terminal_context(&session, &runtime, shell.policy_mode);
        let Some(input) = read_inline_prompt(&context, &next_prompt_input)? else {
            break;
        };
        next_prompt_input.clear();

        let (exit, preserved_input) =
            handle_inline_submission(&input, &runtime, &action_gate, &mut session, &mut shell)?;
        if exit {
            break;
        }
        next_prompt_input = preserved_input;
    }

    Ok(())
}

fn terminal_context<P>(
    session: &Session,
    runtime: &AgentRuntime<P>,
    policy_mode: PermissionPolicyMode,
) -> TerminalShellContext
where
    P: ControllerProvider,
{
    let mut context = TerminalShellContext::from_session(session);
    context.policy_mode = policy_mode;
    if context.provider.is_none() {
        let request = runtime.provider.request_metadata();
        context.provider = Some(request.provider);
        context.model = request.model;
    }
    context
}

fn print_inline_startup(context: &TerminalShellContext) -> io::Result<()> {
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

fn read_inline_prompt(
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

fn handle_inline_submission<P>(
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
        TerminalCommand::Status => {
            print_and_record_local(shell, render_session_status(session))?;
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
    let started = Instant::now();
    let mut tick = 0usize;
    working.render(
        tick,
        started.elapsed().as_secs(),
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
                        started.elapsed().as_secs(),
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
                        started.elapsed().as_secs(),
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
    *session = completed.session;
    shell.consume_events(&completed.events);
    shell.conversation.follow_latest();
    print_new_conversation_lines(shell, before, true, true)?;
    Ok(preserved_input)
}

#[cfg(test)]
fn live_render_due(last_render: Instant, now: Instant) -> bool {
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

fn handle_terminal_input_event(
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
enum ActiveProviderKeyAction {
    Continue,
    Cancel,
    Exit,
}

fn handle_active_provider_key(
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

fn handle_active_provider_input_event(
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

fn print_new_conversation_lines(
    shell: &TuiShell,
    before: usize,
    skip_user_and_loading: bool,
    skip_thinking: bool,
) -> io::Result<()> {
    let lines = shell.conversation.render_lines_with_styles();
    for (line, style) in conversation_print_blocks(
        lines.into_iter().skip(before),
        skip_user_and_loading,
        skip_thinking,
    ) {
        print_conversation_line(&line, style)?;
    }
    io::stdout().flush()
}

fn print_conversation_line(line: &str, style: ConversationLineStyle) -> io::Result<()> {
    match style {
        ConversationLineStyle::User => {
            print_spacer()?;
            let visible = line.strip_prefix("> ").unwrap_or(line);
            print_user_block(visible)
        }
        ConversationLineStyle::Loading => {
            print_spacer()?;
            writeln!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}")
        }
        ConversationLineStyle::Thinking => {
            print_spacer()?;
            writeln!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}")
        }
        ConversationLineStyle::Plain => {
            print_spacer()?;
            print_plain_block(line)
        }
        ConversationLineStyle::Tool => {
            print_spacer()?;
            print_tool_block(line)
        }
    }
}

fn print_spacer() -> io::Result<()> {
    writeln!(io::stdout())
}

fn print_user_block(input: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in non_empty_lines(wrap_words(input, width)) {
        writeln!(
            io::stdout(),
            "{ANSI_USER_BLOCK}{}{ANSI_RESET}",
            pad_line(&line, width)
        )?;
    }
    io::stdout().flush()
}

fn print_plain_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in plain_block_lines(text, width) {
        writeln!(
            io::stdout(),
            "{}{line}{ANSI_RESET}",
            transcript_output_ansi()
        )?;
    }
    io::stdout().flush()
}

fn print_and_record_local(shell: &mut TuiShell, text: impl Into<String>) -> io::Result<()> {
    let text = text.into();
    shell.push_local_message(text.clone());
    print_plain_block(&text)
}

fn print_tool_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in plain_block_lines(text, width) {
        writeln!(
            io::stdout(),
            "{ANSI_TOOL_BLOCK}{}{ANSI_RESET}",
            pad_line(&line, width)
        )?;
    }
    io::stdout().flush()
}

pub fn render_default_terminal_shell(frame: &mut Frame<'_>) {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(".", ".").with_provider("stub-provider", None);
    render_tui_shell(frame, &shell, &context);
}

pub fn render_tui_shell(frame: &mut Frame<'_>, shell: &TuiShell, context: &TerminalShellContext) {
    let area = frame.size();
    let chunks = if shell.pending_action.panel.is_some() {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(7),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(2),
                Constraint::Length(2),
            ])
            .split(area)
    };

    let startup_body = render_terminal_startup(context);
    let conversation_line_count =
        terminal_conversation_line_count(&startup_body, &shell.conversation);
    let conversation_view_height = chunks[0].height;
    let conversation = Paragraph::new(style_terminal_conversation(
        &startup_body,
        &shell.conversation,
        usize::from(chunks[0].width),
    ))
    .style(theme::model_output())
    .wrap(Wrap { trim: false })
    .scroll((
        shell
            .conversation
            .scroll_offset_for_lines(conversation_line_count, conversation_view_height),
        0,
    ));
    frame.render_widget(conversation, chunks[0]);

    let (input_index, status_index) = if shell.pending_action.panel.is_some() {
        let pending = Paragraph::new(shell.pending_action.render_body())
            .style(theme::warning_action())
            .wrap(Wrap { trim: false })
            .block(divider_block("review action"));
        frame.render_widget(pending, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    let input = Paragraph::new(shell.input.render_body())
        .style(theme::user_input_block())
        .block(divider_block(""));
    frame.render_widget(input, chunks[input_index]);

    let status =
        Paragraph::new(context.footer_body_for_width(usize::from(chunks[status_index].width)))
            .style(theme::muted())
            .wrap(Wrap { trim: false })
            .block(Block::default());
    frame.render_widget(status, chunks[status_index]);
}

pub fn default_shell_text() -> String {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(".", ".").with_provider("stub-provider", None);
    format!(
        "{}\n{}\n{}\n{}",
        render_terminal_conversation(&shell, &context),
        shell.input.render_body(),
        context.footer_body(&shell.status.render_body(), &shell.copy.render_hint()),
        default_no_network_line()
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalShellContext {
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub provider_metrics: Option<ProviderMetrics>,
    pub context_accounting: ContextAccounting,
    pub policy_mode: PermissionPolicyMode,
}

impl TerminalShellContext {
    pub fn new(project_root: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            cwd: cwd.as_ref().to_path_buf(),
            provider: None,
            model: None,
            provider_metrics: None,
            context_accounting: ContextAccounting::unknown(),
            policy_mode: PermissionPolicyMode::AutoCreateReviewModify,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>, model: Option<String>) -> Self {
        self.provider = Some(provider.into());
        self.model = model;
        self
    }

    pub fn from_session(session: &Session) -> Self {
        let mut context = Self::new(&session.project_root, &session.cwd);
        if let Some(metadata) = session.provider_metadata() {
            context.provider = Some(metadata.provider.clone());
            context.model = metadata.model.clone();
            context.provider_metrics = metadata.metrics.clone();
        }
        context.context_accounting = session.context_accounting().clone();
        context
    }

    pub fn with_context_accounting(mut self, context_accounting: ContextAccounting) -> Self {
        self.context_accounting = context_accounting;
        self
    }

    pub fn with_policy_mode(mut self, policy_mode: PermissionPolicyMode) -> Self {
        self.policy_mode = policy_mode;
        self
    }

    #[cfg(test)]
    pub fn with_provider_metrics(mut self, provider_metrics: ProviderMetrics) -> Self {
        self.provider_metrics = Some(provider_metrics);
        self
    }

    fn footer_body(&self, _status: &str, _copy_hint: &str) -> String {
        self.footer_body_for_width(80)
    }

    fn footer_body_for_width(&self, width: usize) -> String {
        let left = footer_location_label(&self.project_root, &self.cwd);
        let right = self
            .model
            .as_deref()
            .or(self.provider.as_deref())
            .unwrap_or("");
        let first_line = if right.is_empty() {
            left
        } else {
            align_footer_line(&left, right, width)
        };

        first_line
    }
}

fn render_terminal_conversation(shell: &TuiShell, context: &TerminalShellContext) -> String {
    let startup = render_terminal_startup(context);
    format!("{}\n\n{}", startup, shell.conversation.render_body())
}

fn render_terminal_startup(context: &TerminalShellContext) -> String {
    let startup = StartupBlock::from_context_accounting(
        context.provider.clone(),
        context.model.clone(),
        context.policy_mode,
        &context.context_accounting,
    );
    startup.render()
}

fn terminal_conversation_line_count(startup: &str, conversation: &ConversationPane) -> usize {
    startup.lines().count() + 2 + conversation.render_lines_with_styles().len()
}

fn style_terminal_conversation(
    startup: &str,
    conversation: &ConversationPane,
    width: usize,
) -> Text<'static> {
    let mut lines = startup
        .lines()
        .map(|line| Line::raw(line.to_string()))
        .collect::<Vec<_>>();
    lines.push(Line::raw(String::new()));
    lines.push(Line::raw(String::new()));

    lines.extend(conversation.render_lines_with_styles().into_iter().map(
        |(line, style)| match style {
            ConversationLineStyle::User => {
                let visible = line.strip_prefix("> ").unwrap_or(&line);
                Line::styled(pad_line(visible, width), theme::user_input_block())
            }
            ConversationLineStyle::Loading => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Thinking => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Plain => Line::styled(line, theme::model_output()),
            ConversationLineStyle::Tool => {
                Line::styled(pad_line(&line, width), theme::tool_output())
            }
        },
    ));

    Text::from(lines)
}

fn default_no_network_line() -> &'static str {
    "default no-network stub"
}

fn divider_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::TOP)
        .title_style(theme::accent())
        .border_style(theme::muted())
}

#[cfg(test)]
fn status_style(status: &str) -> ratatui::style::Style {
    if status.contains("error") || status.starts_with("failed") {
        theme::error()
    } else if status.starts_with("thinking") || status.contains("working") {
        theme::thinking()
    } else if status.starts_with("applied") || status == "reply ready" || status == "ready" {
        theme::success()
    } else if status.starts_with("review")
        || status.starts_with("approved")
        || status.starts_with("rejected")
    {
        theme::warning_action()
    } else {
        theme::muted()
    }
}

#[cfg(test)]
fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    key.modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL)
        && matches!(
            key.code,
            crossterm::event::KeyCode::Char('c') | crossterm::event::KeyCode::Char('d')
        )
}

#[cfg(test)]
fn handle_terminal_key<P>(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> bool
where
    P: ControllerProvider + Clone,
{
    handle_terminal_key_with_copy_writer(key, input, controller, session, shell, io::stdout())
}

#[cfg(test)]
fn handle_terminal_key_with_copy_writer<P>(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    copy_writer: impl Write,
) -> bool
where
    P: ControllerProvider + Clone,
{
    if should_exit(key) {
        return true;
    }

    if handle_scroll_key(key, shell) {
        return false;
    }

    match input.handle_key(key) {
        TerminalInputAction::Continue => false,
        TerminalInputAction::Exit => true,
        TerminalInputAction::Submit => {
            let submitted = input.drain();
            shell.input.text.clear();
            let runtime = AgentRuntime::new(controller.provider.clone());
            let action_gate = ActionGate::new(controller.provider.clone());
            handle_submitted_terminal_input(
                &submitted,
                &runtime,
                &action_gate,
                session,
                shell,
                copy_writer,
            )
        }
    }
}

fn handle_submitted_terminal_input<P>(
    submitted: &str,
    runtime: &AgentRuntime<P>,
    action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    copy_writer: impl Write,
) -> bool
where
    P: ControllerProvider,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Empty => {}
        TerminalCommand::Help => {
            shell
                .conversation
                .lines
                .push(render_terminal_help().to_string());
            shell.conversation.follow_latest();
        }
        TerminalCommand::Clear => {
            clear_terminal_conversation(shell);
        }
        TerminalCommand::Approve => {
            shell.submit_approval(action_gate, session);
        }
        TerminalCommand::Reject => {
            shell.submit_rejection(action_gate, session);
        }
        TerminalCommand::Cancel => {
            shell
                .conversation
                .push_local_message("No provider request is running.");
            shell.conversation.follow_latest();
        }
        TerminalCommand::Memory => {
            shell
                .conversation
                .push_local_message(render_session_memory(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::State => {
            shell
                .conversation
                .push_local_message(render_session_state_snapshot(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::PlanPreview => {
            shell
                .conversation
                .push_local_message(render_session_plan_preview(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Status => {
            shell
                .conversation
                .push_local_message(render_session_status(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Pending => {
            shell
                .conversation
                .push_local_message(render_session_pending_action(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Created => {
            shell
                .conversation
                .push_local_message(render_session_created_actions(session));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Permissions(argument) => {
            let message = shell.apply_permission_command(argument);
            shell.push_local_message(message);
        }
        TerminalCommand::Tool(text) => {
            handle_terminal_tool_input(text, runtime, action_gate, session, shell);
        }
        TerminalCommand::Copy => {
            let _ = copy_conversation_to_terminal_clipboard(copy_writer, shell);
        }
        TerminalCommand::Exit => return true,
        TerminalCommand::Unknown(command) => {
            shell.conversation.push_local_message(format!(
                "Unknown command: {command}. Type /commands for commands."
            ));
            shell.conversation.follow_latest();
        }
        TerminalCommand::Text(text) => {
            handle_terminal_text_input(text, runtime, action_gate, session, shell);
        }
    }
    false
}

fn handle_terminal_text_input<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    _action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) where
    P: ControllerProvider,
{
    shell.submit_agent_input(runtime, session, text);
}

fn handle_terminal_tool_input<P>(
    text: &str,
    runtime: &AgentRuntime<P>,
    _action_gate: &ActionGate<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) where
    P: ControllerProvider,
{
    shell.submit_agent_tool_input(runtime, session, text);
}

fn terminal_text_should_run_inline_provider_text(_text: &str) -> bool {
    true
}

fn normalize_terminal_provider_text_input(text: &str) -> String {
    normalize_pasted_transcript_input(text).trim().to_string()
}

#[cfg(test)]
fn handle_submitted_terminal_input_for_loop<P>(
    submitted: &str,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    pending_turn: &mut Option<ProviderTurnTask>,
) -> bool
where
    P: ControllerProvider + Clone + Send + 'static,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Cancel => {
            if let Some(task) = pending_turn.take() {
                task.cancel();
                shell.conversation.discard_pending_provider_turn();
                shell
                    .conversation
                    .push_local_message("Provider request canceled.");
                shell.conversation.follow_latest();
                shell.status.cancel_provider_turn();
            } else {
                shell
                    .conversation
                    .push_local_message("No provider request is running.");
                shell.conversation.follow_latest();
            }
            false
        }
        TerminalCommand::Text(text) if terminal_text_should_run_inline_provider_text(text) => {
            let runtime = AgentRuntime::new(controller.provider.clone());
            let provider_input = normalize_terminal_provider_text_input(text);
            shell
                .conversation
                .push_pending_provider_turn(&provider_input);
            shell.conversation.follow_latest();
            shell.status.start_thinking_pulse();
            *pending_turn = Some(start_provider_text_turn(
                runtime,
                session.clone(),
                provider_input,
                shell.policy_mode,
            ));
            false
        }
        TerminalCommand::Tool(text) => {
            let runtime = AgentRuntime::new(controller.provider.clone());
            shell
                .conversation
                .push_pending_provider_turn(&format!("/tool {text}"));
            shell.conversation.follow_latest();
            shell.status.start_thinking_pulse();
            *pending_turn = Some(start_tool_turn(
                runtime,
                session.clone(),
                text.to_string(),
                shell.policy_mode,
            ));
            false
        }
        _ => {
            let runtime = AgentRuntime::new(controller.provider.clone());
            let action_gate = ActionGate::new(controller.provider.clone());
            handle_submitted_terminal_input(
                submitted,
                &runtime,
                &action_gate,
                session,
                shell,
                io::stdout(),
            )
        }
    }
}

#[cfg(test)]
fn handle_scroll_key(key: crossterm::event::KeyEvent, shell: &mut TuiShell) -> bool {
    match key.code {
        crossterm::event::KeyCode::PageUp => {
            shell.conversation.scroll_up(5);
            true
        }
        crossterm::event::KeyCode::PageDown => {
            shell.conversation.scroll_down(5);
            true
        }
        crossterm::event::KeyCode::End
            if key
                .modifiers
                .contains(crossterm::event::KeyModifiers::CONTROL) =>
        {
            shell.conversation.follow_latest();
            true
        }
        _ => false,
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

#[cfg(test)]
mod tests;
