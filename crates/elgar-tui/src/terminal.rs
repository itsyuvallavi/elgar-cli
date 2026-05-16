use std::{
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use elgar_core::{
    provider::{ControllerProvider, ProviderConfig},
    session::Session,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};

use elgar_core::controller::Controller;

use crate::{
    input::{TerminalInput, TerminalInputAction},
    TuiShell,
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
    P: ControllerProvider,
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
    let context = TerminalShellContext::new(".", ".");
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

    let conversation_view_height = chunks[0].height;
    let conversation = Paragraph::new(shell.conversation.render_body())
        .style(Style::default().fg(Color::Gray))
        .wrap(Wrap { trim: false })
        .scroll((
            shell.conversation.scroll_offset(conversation_view_height),
            0,
        ));
    frame.render_widget(conversation, chunks[0]);

    let (input_index, status_index) = if shell.pending_action.panel.is_some() {
        let pending = Paragraph::new(shell.pending_action.render_body())
            .style(Style::default().fg(Color::Gray))
            .wrap(Wrap { trim: false })
            .block(divider_block("review action"));
        frame.render_widget(pending, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    let input = Paragraph::new(shell.input.render_body())
        .style(Style::default().fg(Color::Cyan))
        .block(divider_block(""));
    frame.render_widget(input, chunks[input_index]);

    let status =
        Paragraph::new(context.footer_body(&shell.status.render_body(), &shell.copy.render_hint()))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false })
            .block(Block::default());
    frame.render_widget(status, chunks[status_index]);
}

pub fn default_shell_text() -> String {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(".", ".");
    format!(
        "{}\n{}\n{}\n{}",
        shell.conversation.render_body(),
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

    pub fn from_session(session: &Session) -> Self {
        let mut context = Self::new(&session.project_root, &session.cwd);
        if let Some(metadata) = session.provider_metadata() {
            context.provider = Some(metadata.provider.clone());
            context.model = metadata.model.clone();
        }
        context
    }

    fn footer_body(&self, status: &str, copy_hint: &str) -> String {
        format!(
            "{} | proj:{} | cwd:{} | prov:{} | model:{} | {} | {}",
            status,
            compact_path_label(&self.project_root),
            compact_cwd_label(&self.project_root, &self.cwd),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none"),
            self.provider_mode_label(),
            copy_hint
        )
    }

    fn provider_mode_label(&self) -> &'static str {
        match self.provider.as_deref() {
            Some("lm-studio") => "live/local",
            Some("stub-provider") | None => compact_no_network_label(),
            Some(_) => "provider configured",
        }
    }
}

fn default_no_network_line() -> &'static str {
    "default no-network stub"
}

fn compact_no_network_label() -> &'static str {
    "stub/no-network"
}

fn compact_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn compact_cwd_label(project_root: &Path, cwd: &Path) -> String {
    if cwd == project_root {
        ".".to_string()
    } else {
        compact_path_label(cwd)
    }
}

fn divider_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray))
}

fn run_terminal_loop<P>(
    terminal: &mut CrosstermTerminal,
    controller: &Controller<P>,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<()>
where
    P: ControllerProvider,
{
    let mut input = TerminalInput::default();

    loop {
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
                    if handle_terminal_key(key, &mut input, controller, session, shell) {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc)
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

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
            match parse_terminal_command(&submitted) {
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
                    shell.conversation.lines.push(format!(
                        "Unknown command: {command}. Type /help for commands."
                    ));
                    shell.conversation.follow_latest();
                }
                TerminalCommand::Text(text) => {
                    shell.submit_input(controller, session, text);
                }
            }
            false
        }
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
    "Elgar terminal commands:\n  /help      Show these commands.\n  /commands  Show these commands.\n  /approve   Approve the pending action.\n  /reject    Reject the pending action.\n  /copy      Copy the full conversation.\n  /exit      Exit the TUI.\n  /quit      Exit the TUI."
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
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(guard)
    }
}

impl Drop for TerminalModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::{action::ActionLifecycleState, controller::Controller, session::Session};
    use ratatui::{backend::TestBackend, Terminal};

    use crate::{input::TerminalInput, TuiShell};

    use super::{
        copy_conversation_to_terminal_clipboard, default_shell_text, encode_base64,
        handle_scroll_key, handle_terminal_key, handle_terminal_key_with_copy_writer,
        osc52_clipboard_sequence, parse_terminal_command, render_terminal_help, render_tui_shell,
        should_exit, TerminalCommand, TerminalShellContext,
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

        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("> "));
        assert!(text.contains("ready"));
        assert!(text.contains("prov:none"));
        assert!(text.contains("model:none"));
        assert!(text.contains("select visible text natively"));
        assert!(text.contains("/copy conversation"));
        assert!(!text.contains("Ctrl+Y copy conversation"));
        assert!(!text.contains("br:"));
        assert!(text.contains("default no-network stub"));
        assert!(!text.contains("lm-studio"));
    }

    #[test]
    fn terminal_layout_renders_default_shell_regions() {
        let shell = TuiShell::new();
        let context = TerminalShellContext::new("/repo", "/repo/crates");
        let text = draw_to_text(&shell, &context);

        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("> "));
        assert!(text.contains("proj:repo"));
        assert!(text.contains("cwd:crates"));
        assert!(text.contains("select visible text"));
        assert!(!text.contains("br:"));
        assert!(text.contains("prov:none"));
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

        assert!(text.contains("prov:local"));
        assert!(text.contains("model:model-a"));
        assert!(footer.contains("provider configured"));
        assert!(!footer.contains("stub/no-network"));
        assert!(text.contains("Provider progress: response ready from local"));
    }

    #[test]
    fn terminal_footer_labels_lm_studio_as_live_local() {
        let mut context = TerminalShellContext::new("/repo", "/repo");
        context.provider = Some("lm-studio".to_string());
        context.model = Some("openai/gpt-oss-20b".to_string());

        let footer = context.footer_body("ready", "select visible text");

        assert!(footer.contains("prov:lm-studio"));
        assert!(footer.contains("model:openai/gpt-oss-20b"));
        assert!(footer.contains("live/local"));
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

        assert!(text.contains("line 0"));
        assert!(!text.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(text.contains("review action"));
        assert!(text.contains("Action: action-1 WriteFile"));
        assert!(text.contains("> "));
        assert!(text.contains("proj:repo"));
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
        assert!(help.contains("/help"));
        assert!(help.contains("/commands"));
        assert!(help.contains("/approve"));
        assert!(help.contains("/reject"));
        assert!(help.contains("/copy"));
        assert!(help.contains("/exit"));
        assert!(help.contains("/quit"));
        assert!(!help.contains("/model"));
        assert!(!help.contains("/settings"));
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
        assert!(shell.render().contains("You: what does the harness do?"));
        assert!(shell
            .render()
            .contains("Assistant suggestion: stub provider response"));
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
        assert!(rendered.contains("You: hello!"));
        assert!(rendered.contains("Assistant suggestion:"));
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
        assert!(shell.copy.render_hint().contains("/copy conversation"));

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
