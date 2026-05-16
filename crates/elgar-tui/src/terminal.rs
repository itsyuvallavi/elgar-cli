use std::{
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode},
};
use elgar_core::{
    provider::{ControllerProvider, ProviderConfig},
    router::{route_input, Route},
    session::Session,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

use elgar_core::controller::Controller;

use crate::{
    input::{TerminalInput, TerminalInputAction},
    panes::{ConversationLineStyle, ConversationPane},
    startup::StartupBlock,
    theme, TuiShell,
};

type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

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
    let _guard = TerminalModeGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut session = Session::new("terminal-tui-session", project_root.as_ref(), cwd.as_ref());
    let mut shell = TuiShell::new();

    let result = run_terminal_loop(&mut terminal, &controller, &mut session, &mut shell);
    let cursor_result = terminal.show_cursor();

    result.and(cursor_result)
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
    } else if status.starts_with("thinking") {
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

fn run_terminal_loop<P>(
    terminal: &mut CrosstermTerminal,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<()>
where
    P: ControllerProvider + Clone + Send + 'static,
{
    let mut input = TerminalInput::default();
    let mut pending_turn: Option<ProviderTurnTask> = None;

    loop {
        if let Some(task) = pending_turn.take() {
            match task.try_complete() {
                Ok(Some(completed)) => {
                    *session = completed.session;
                    shell.conversation.discard_pending_provider_turn();
                    shell.consume_events(&completed.events);
                    shell.conversation.follow_latest();
                }
                Ok(None) => pending_turn = Some(task),
                Err(message) => shell.status.finish_with_error(message),
            }
        }

        shell.status.advance_thinking_pulse();
        shell.conversation.advance_loading_pulse();
        shell.input.text = input.text().to_string();
        let mut context = TerminalShellContext::from_session(session);
        if context.provider.is_none() {
            let request = controller.provider.request_metadata();
            context.provider = Some(request.provider);
            context.model = request.model;
        }
        terminal.draw(|frame| render_tui_shell(frame, shell, &context))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if pending_turn.is_some() {
                        if handle_terminal_key_while_provider_active(key, &mut input, shell) {
                            break;
                        }
                    } else if handle_terminal_key_for_loop(
                        key,
                        &mut input,
                        controller,
                        session,
                        shell,
                        &mut pending_turn,
                    ) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
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
        let result = controller.turn(&mut session, &input);
        let _ = sender.send(Ok(CompletedProviderTurn {
            session,
            events: result.events,
        }));
    });

    ProviderTurnTask { receiver }
}

fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc)
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
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

fn handle_terminal_key_for_loop<P>(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
    pending_turn: &mut Option<ProviderTurnTask>,
) -> bool
where
    P: ControllerProvider + Clone + Send + 'static,
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
            handle_submitted_terminal_input_for_loop(
                &submitted,
                controller,
                session,
                shell,
                pending_turn,
            )
        }
    }
}

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

    if matches!(key.code, KeyCode::Enter) {
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
        TerminalCommand::Text(text) if route_input(text) == Route::AskModel => {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalCommand<'a> {
    Empty,
    Help,
    Approve,
    Reject,
    Copy,
    Exit,
    Unknown(&'a str),
    Text(&'a str),
}

fn parse_terminal_command(input: &str) -> TerminalCommand<'_> {
    let trimmed = input.trim();
    match trimmed {
        "" => TerminalCommand::Empty,
        "/help" | "/commands" => TerminalCommand::Help,
        "/approve" => TerminalCommand::Approve,
        "/reject" => TerminalCommand::Reject,
        "/copy" => TerminalCommand::Copy,
        "/exit" | "/quit" => TerminalCommand::Exit,
        command if command.starts_with('/') => TerminalCommand::Unknown(command),
        text => TerminalCommand::Text(text),
    }
}

fn render_terminal_help() -> &'static str {
    "Commands\n/commands  Show commands\n/approve   Apply the pending action\n/reject    Reject the pending action\n/copy      Copy the conversation\n/exit      Quit\n/quit      Quit\n/help      Show commands"
}

fn handle_scroll_key(key: crossterm::event::KeyEvent, shell: &mut TuiShell) -> bool {
    match key.code {
        KeyCode::PageUp => {
            shell.conversation.scroll_up(5);
            true
        }
        KeyCode::PageDown => {
            shell.conversation.scroll_down(5);
            true
        }
        KeyCode::End if key.modifiers.contains(KeyModifiers::CONTROL) => {
            shell.conversation.follow_latest();
            true
        }
        _ => false,
    }
}

fn copy_conversation_to_terminal_clipboard(
    mut writer: impl Write,
    shell: &mut TuiShell,
) -> io::Result<()> {
    let text = shell.conversation_copy_text();
    let result = writer
        .write_all(osc52_clipboard_sequence(&text).as_bytes())
        .and_then(|_| writer.flush());

    match result {
        Ok(()) => {
            shell.copy.mark_copied(text.len());
            Ok(())
        }
        Err(error) => {
            shell.copy.mark_failed(error.to_string());
            Err(error)
        }
    }
}

fn osc52_clipboard_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", encode_base64(text.as_bytes()))
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let triple = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);

        encoded.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}

struct TerminalModeGuard;

impl TerminalModeGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::{action::ActionLifecycleState, controller::Controller, session::Session};
    use ratatui::{backend::TestBackend, Terminal};

    use crate::{input::TerminalInput, panes::ConversationPane, TuiShell};

    use super::{
        copy_conversation_to_terminal_clipboard, default_shell_text, encode_base64,
        handle_scroll_key, handle_submitted_terminal_input_for_loop, handle_terminal_key,
        handle_terminal_key_while_provider_active, handle_terminal_key_with_copy_writer,
        osc52_clipboard_sequence, parse_terminal_command, render_terminal_help, render_tui_shell,
        should_exit, status_style, style_terminal_conversation, TerminalCommand,
        TerminalShellContext,
    };

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

    #[test]
    fn default_terminal_shell_is_empty_and_no_network() {
        let text = default_shell_text();

        assert!(text.contains("elgar v0.2"));
        assert!(text.contains("/commands · /approve · /reject · /copy · /exit"));
        assert!(text.contains(
            "Elgar uses your local LM Studio model and keeps file changes behind approval."
        ));
        assert!(text.contains("[Context]"));
        assert!(text.contains("[Provider]\n  stub-provider · none"));
        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("> "));
        assert!(text.contains("context: TBD"));
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
            .with_provider("lm-studio", Some("openai/gpt-oss-20b".to_string()));

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
        assert!(text.contains("context: TBD"));
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

        assert!(text.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(text.contains("review action"));
        assert!(text.contains("Action: action-1 WriteFile"));
        assert!(text.contains("State: waiting for approval"));
        assert!(text.contains("No file has been changed yet"));
        assert!(text.contains("Use /approve or /reject"));
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
        assert!(footer.contains("context: TBD"));
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

        assert!(footer.contains(&format!("{}", root.file_name().unwrap().to_str().unwrap())));
        assert!(footer.contains("crates/elgar-tui"));
        assert!(footer.contains("(feature/footer)"));
        assert!(footer.contains("openai/gpt-oss-20b"));
        assert!(footer.contains("context: TBD"));
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
        assert_eq!(shell.status.render_body(), "thinking");
        assert!(shell.status.provider_active());
        assert!(shell
            .conversation
            .render_body()
            .contains("> what does the harness do?\nthinking"));
        shell.status.advance_thinking_pulse();
        shell.conversation.advance_loading_pulse();
        assert_eq!(shell.status.render_body(), "thinking.");
        assert!(shell.conversation.render_body().contains("thinking."));

        let task = pending_turn.take().unwrap();
        let completed = (0..20)
            .find_map(|_| {
                let result = task.try_complete().unwrap();
                if result.is_none() {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                result
            })
            .expect("stub provider turn should complete");

        session = completed.session;
        shell.conversation.discard_pending_provider_turn();
        shell.consume_events(&completed.events);

        assert_eq!(session.events().len(), completed.events.len());
        assert_eq!(shell.status.render_body(), "reply ready");
        assert!(!shell.status.provider_active());
        assert!(!shell.render().contains("User\n"));
        assert!(shell.render().contains("Model: stub provider response"));
    }

    #[test]
    fn terminal_status_uses_named_theme_styles_by_state() {
        assert_eq!(status_style("ready"), crate::theme::success());
        assert_eq!(status_style("reply ready"), crate::theme::success());
        assert_eq!(status_style("thinking..."), crate::theme::thinking());
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
        assert!(footer.contains("context: TBD"));
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
        assert!(!text.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(text.contains("review action"));
        assert!(text.contains("Action: action-1 WriteFile"));
        assert!(text.contains("> "));
        assert!(text.contains("repo"));
    }

    #[test]
    fn terminal_shell_exit_keys_are_minimal() {
        assert!(should_exit(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE
        )));
        assert!(should_exit(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
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
        assert_eq!(parse_terminal_command("/approve"), TerminalCommand::Approve);
        assert_eq!(parse_terminal_command("/reject"), TerminalCommand::Reject);
        assert_eq!(parse_terminal_command("/copy"), TerminalCommand::Copy);
        assert_eq!(parse_terminal_command("/exit"), TerminalCommand::Exit);
        assert_eq!(parse_terminal_command("/quit"), TerminalCommand::Exit);
        assert_eq!(
            parse_terminal_command("/model"),
            TerminalCommand::Unknown("/model")
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
        assert!(help.contains("/approve"));
        assert!(help.contains("/reject"));
        assert!(help.contains("/copy"));
        assert!(help.contains("/exit"));
        assert!(help.contains("/quit"));
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
    fn terminal_rejects_stale_input_while_provider_is_active() {
        let mut input = TerminalInput::default();
        let mut shell = TuiShell::new();
        shell.status.start_thinking_pulse();

        for character in "/approve".chars() {
            let exited = handle_terminal_key_while_provider_active(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char(character),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut input,
                &mut shell,
            );
            assert!(!exited);
        }

        assert!(input.text().is_empty());

        let exited = handle_terminal_key_while_provider_active(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &mut shell,
        );

        assert!(!exited);
        assert!(input.text().is_empty());
        assert!(shell.status.provider_active());
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
        assert!(shell.render().contains("Model: stub provider response"));
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
        assert!(rendered.contains("Model:"));
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
        assert!(shell.render().contains("State: applied and verified"));
        assert!(shell.render().contains("Result: file written:"));

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
        assert!(shell.render().contains("State: rejected"));
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
        let before_session;
        let mut shell = TuiShell::new();
        let mut input = TerminalInput::default();

        shell.submit_input(&controller, &mut session, "create file approved.py");
        before_session = session.clone();

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
            let exited =
                handle_terminal_key(key, &mut input, &controller, &mut session, &mut shell);
            assert!(!exited);
        }

        assert!(!target.exists());
        assert_eq!(session, before_session);
        assert_eq!(input.text(), "q");
        assert_eq!(shell.copy.render_hint(), "");

        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-terminal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
