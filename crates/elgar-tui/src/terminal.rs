use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyEventKind},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use elgar_core::{
    provider::{ControllerProvider, ProviderConfig},
    router::{route_input, Route},
    session::Session,
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use elgar_core::controller::Controller;

use crate::{
    input::{TerminalInput, TerminalInputAction},
    panes::{ConversationLineStyle, ConversationPane},
    startup::StartupBlock,
    theme, TuiShell,
};

mod commands;
mod prompt;

use commands::{
    clear_terminal_conversation, clear_visible_terminal, copy_conversation_to_terminal_clipboard,
    parse_terminal_command, render_terminal_help, terminal_text_starts_provider_turn,
    TerminalCommand,
};
#[cfg(test)]
use commands::{encode_base64, osc52_clipboard_sequence};
#[cfg(test)]
use prompt::{active_working_frame_lines, inline_prompt_frame_lines};
use prompt::{
    drawable_width, frame_separator_line, non_empty_lines, terminal_width, wrap_words,
    InlinePromptRenderer, InlineWorkingRenderer,
};

const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_CYAN: &str = "\x1b[38;2;143;207;198m";
const ANSI_MUTED: &str = "\x1b[38;2;118;126;126m";
const ANSI_TEXT: &str = "\x1b[38;2;214;219;224m";
const ANSI_USER_BLOCK: &str = "\x1b[1m\x1b[38;2;143;207;198m\x1b[48;2;8;32;32m";
const ANSI_CURSOR_HIDE: &str = "\x1b[?25l";
const ANSI_CURSOR_SHOW: &str = "\x1b[?25h";

pub fn run_terminal_shell() -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_at(&cwd, &cwd)
}

pub fn run_terminal_shell_with_lm_studio_provider(config: ProviderConfig) -> io::Result<()> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    run_terminal_shell_with_controller(&cwd, &cwd, Controller::with_lm_studio_provider(config))
}

pub fn run_terminal_shell_at(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    run_terminal_shell_with_controller(project_root, cwd, Controller::default())
}

fn run_terminal_shell_with_controller<P>(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
    controller: Controller<P>,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let mut session = Session::new("terminal-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = TuiShell::new();

    let mut context = terminal_context(&session, &controller);
    print_inline_startup(&context)?;

    loop {
        context = terminal_context(&session, &controller);
        let Some(input) = read_inline_prompt(&context)? else {
            break;
        };

        if handle_inline_submission(&input, &controller, &mut session, &mut shell)? {
            break;
        }
    }

    Ok(())
}

fn terminal_context<P>(session: &Session, controller: &Controller<P>) -> TerminalShellContext
where
    P: ControllerProvider,
{
    let mut context = TerminalShellContext::from_session(session);
    if context.provider.is_none() {
        let request = controller.provider.request_metadata();
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

fn read_inline_prompt(context: &TerminalShellContext) -> io::Result<Option<String>> {
    let _guard = TerminalModeGuard::enter()?;
    let mut input = TerminalInput::default();
    let mut renderer = InlinePromptRenderer::new(context.clone());
    renderer.render(input.text())?;

    loop {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match input.handle_key(key) {
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
            },
            _ => {}
        }
    }
}

fn handle_inline_submission<P>(
    submitted: &str,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<bool>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    match parse_terminal_command(submitted) {
        TerminalCommand::Empty => Ok(false),
        TerminalCommand::Exit => Ok(true),
        TerminalCommand::Help => {
            print_plain_block(render_terminal_help())?;
            Ok(false)
        }
        TerminalCommand::Clear => {
            clear_terminal_conversation(shell);
            clear_visible_terminal()?;
            Ok(false)
        }
        TerminalCommand::Copy => {
            let mut sink = io::stdout();
            let _ = copy_conversation_to_terminal_clipboard(&mut sink, shell);
            if !shell.copy.render_hint().is_empty() {
                print_plain_block(&shell.copy.render_hint())?;
            }
            Ok(false)
        }
        TerminalCommand::Unknown(command) => {
            print_plain_block(&format!(
                "Unknown command: {command}. Type /commands for commands."
            ))?;
            Ok(false)
        }
        TerminalCommand::Approve | TerminalCommand::Reject => {
            let before = shell.conversation.render_lines_with_styles().len();
            let exit = handle_submitted_terminal_input(
                submitted,
                controller,
                session,
                shell,
                io::stdout(),
            );
            print_new_conversation_lines(shell, before, false)?;
            Ok(exit)
        }
        TerminalCommand::Text(text) if terminal_text_starts_provider_turn(text) => {
            run_inline_provider_turn(text, controller, session, shell)?;
            Ok(false)
        }
        TerminalCommand::Text(text) => {
            let before = shell.conversation.render_lines_with_styles().len();
            handle_terminal_text_input(text, controller, session, shell);
            print_new_conversation_lines(shell, before, false)?;
            Ok(false)
        }
    }
}

fn run_inline_provider_turn<P>(
    text: &str,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let before = shell.conversation.render_lines_with_styles().len();
    print_spacer()?;
    print_user_block(text)?;

    let task = start_provider_turn(controller.clone(), session.clone(), text.to_string());
    let mut working = InlineWorkingRenderer::new(terminal_context(session, controller));
    let started = Instant::now();
    let mut tick = 0usize;

    let completed = loop {
        match task.try_complete() {
            Ok(Some(completed)) => break completed,
            Ok(None) => {
                working.render(tick, started.elapsed().as_secs())?;
                tick = tick.wrapping_add(1);
                thread::sleep(Duration::from_millis(420));
            }
            Err(message) => {
                working.clear()?;
                print_plain_block(&format!("Provider error: {message}"))?;
                return Ok(());
            }
        }
    };

    working.clear()?;
    *session = completed.session;
    shell.consume_events(&completed.events);
    shell.conversation.follow_latest();
    print_new_conversation_lines(shell, before, true)?;
    Ok(())
}

fn print_new_conversation_lines(
    shell: &TuiShell,
    before: usize,
    skip_user_and_loading: bool,
) -> io::Result<()> {
    let lines = shell.conversation.render_lines_with_styles();
    for (line, style) in lines.into_iter().skip(before) {
        if skip_user_and_loading
            && matches!(
                style,
                ConversationLineStyle::User | ConversationLineStyle::Loading
            )
        {
            continue;
        }
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
    for line in non_empty_lines(wrap_words(text, width)) {
        writeln!(io::stdout(), "{ANSI_TEXT}{line}{ANSI_RESET}")?;
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
}

impl TerminalShellContext {
    pub fn new(project_root: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            cwd: cwd.as_ref().to_path_buf(),
            provider: None,
            model: None,
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
        }
        context
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

        format!("{first_line}\ncontext: TBD")
    }
}

fn render_terminal_conversation(shell: &TuiShell, context: &TerminalShellContext) -> String {
    let startup = render_terminal_startup(context);
    format!("{}\n\n{}", startup, shell.conversation.render_body())
}

fn render_terminal_startup(context: &TerminalShellContext) -> String {
    let startup = StartupBlock::new(
        &context.project_root,
        &context.cwd,
        context.provider.clone(),
        context.model.clone(),
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
            ConversationLineStyle::Plain => Line::raw(line),
        },
    ));

    Text::from(lines)
}

fn pad_line(line: &str, width: usize) -> String {
    let visible_width = line.chars().count();
    if visible_width >= width {
        line.to_string()
    } else {
        format!("{line}{:padding$}", "", padding = width - visible_width)
    }
}

fn default_no_network_line() -> &'static str {
    "default no-network stub"
}

fn footer_location_label(project_root: &Path, cwd: &Path) -> String {
    let mut parts = vec![project_footer_label(project_root)];
    if cwd != project_root {
        parts.push(compact_cwd_label(project_root, cwd));
    }
    if let Some(branch) = current_git_branch(project_root) {
        parts.push(format!("({branch})"));
    }
    parts.join(" ")
}

fn align_footer_line(left: &str, right: &str, width: usize) -> String {
    let left_width = left.chars().count();
    let right_width = right.chars().count();
    let minimum_gap = 2;
    if width > left_width + right_width + minimum_gap {
        format!(
            "{left}{:gap$}{right}",
            "",
            gap = width - left_width - right_width
        )
    } else {
        format!("{left}  {right}")
    }
}

fn current_git_branch(project_root: &Path) -> Option<String> {
    let head = std::fs::read_to_string(project_root.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return non_empty_label(branch);
    }
    None
}

fn non_empty_label(label: &str) -> Option<String> {
    let label = label.trim();
    if label.is_empty() {
        None
    } else {
        Some(label.to_string())
    }
}

fn compact_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn project_footer_label(project_root: &Path) -> String {
    if let Some(home_label) = home_relative_label(project_root) {
        return home_label;
    }
    compact_repo_label(project_root)
}

fn home_relative_label(path: &Path) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let home = PathBuf::from(home);
    let relative = path.strip_prefix(home).ok()?;
    let label = relative.display().to_string();
    if label.is_empty() {
        Some("~".to_string())
    } else {
        Some(format!("~/{}", label))
    }
}

fn compact_repo_label(project_root: &Path) -> String {
    let repo = compact_path_label(project_root);
    let Some(parent) = project_root.parent() else {
        return repo;
    };
    let Some(parent_name) = parent.file_name().and_then(|name| name.to_str()) else {
        return repo;
    };
    if parent_name.is_empty() {
        repo
    } else {
        format!("{parent_name}/{repo}")
    }
}

fn compact_cwd_label(project_root: &Path, cwd: &Path) -> String {
    if cwd == project_root {
        ".".to_string()
    } else if let Ok(relative) = cwd.strip_prefix(project_root) {
        let label = relative.display().to_string();
        if label.is_empty() {
            ".".to_string()
        } else {
            label
        }
    } else {
        compact_path_label(cwd)
    }
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

struct ProviderTurnTask {
    receiver: mpsc::Receiver<Result<CompletedProviderTurn, String>>,
}

impl ProviderTurnTask {
    fn try_complete(&self) -> Result<Option<CompletedProviderTurn>, String> {
        match self.receiver.try_recv() {
            Ok(result) => result.map(Some),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("provider request worker disconnected".to_string())
            }
        }
    }
}

struct CompletedProviderTurn {
    session: Session,
    events: Vec<elgar_core::event::Event>,
}

fn start_provider_turn<P>(
    controller: Controller<P>,
    mut session: Session,
    input: String,
) -> ProviderTurnTask
where
    P: ControllerProvider + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = controller.model_turn(&mut session, &input);
        let _ = sender.send(Ok(CompletedProviderTurn {
            session,
            events: result.events,
        }));
    });

    ProviderTurnTask { receiver }
}

#[cfg(test)]
fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    matches!(key.code, crossterm::event::KeyCode::Esc)
        || (key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL)
            && key.code == crossterm::event::KeyCode::Char('c'))
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
    P: ControllerProvider,
{
    handle_terminal_key_with_copy_writer(key, input, controller, session, shell, io::stdout())
}

#[cfg(test)]
fn handle_terminal_key_while_provider_active(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    shell: &mut TuiShell,
) -> bool {
    if should_exit(key) {
        return true;
    }

    if handle_scroll_key(key, shell) {
        return false;
    }

    if matches!(key.code, crossterm::event::KeyCode::Enter) {
        let _ = input.drain();
        shell.input.text.clear();
    }

    false
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
    P: ControllerProvider,
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
            handle_submitted_terminal_input(&submitted, controller, session, shell, copy_writer)
        }
    }
}

fn handle_submitted_terminal_input<P>(
    submitted: &str,
    controller: &Controller<P>,
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
            shell.submit_approval(controller, session);
        }
        TerminalCommand::Reject => {
            shell.submit_rejection(controller, session);
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
            handle_terminal_text_input(text, controller, session, shell);
        }
    }
    false
}

fn handle_terminal_text_input<P>(
    text: &str,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) where
    P: ControllerProvider,
{
    match route_input(text) {
        Route::ApproveAction | Route::RejectAction => {
            shell
                .conversation
                .push_local_message("Action commands must use /approve or /reject.");
            shell.conversation.follow_latest();
        }
        Route::Help => {
            shell
                .conversation
                .push_local_message("Type /commands to show available commands.");
            shell.conversation.follow_latest();
        }
        _ => {
            shell.submit_input(controller, session, text);
        }
    }
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
        TerminalCommand::Text(text) if terminal_text_starts_provider_turn(text) => {
            shell.conversation.push_pending_provider_turn(text);
            shell.conversation.follow_latest();
            shell.status.start_thinking_pulse();
            *pending_turn = Some(start_provider_turn(
                controller.clone(),
                session.clone(),
                text.to_string(),
            ));
            false
        }
        _ => handle_submitted_terminal_input(submitted, controller, session, shell, io::stdout()),
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
        write!(io::stdout(), "{ANSI_CURSOR_HIDE}")?;
        io::stdout().flush()?;
        Ok(Self)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _ = write!(io::stdout(), "{ANSI_CURSOR_SHOW}");
        let _ = io::stdout().flush();
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests;
