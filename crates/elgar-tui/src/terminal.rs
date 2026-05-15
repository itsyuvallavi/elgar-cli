use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use elgar_core::session::Session;
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

pub fn run_terminal_shell_at(
    project_root: impl AsRef<Path>,
    cwd: impl AsRef<Path>,
) -> io::Result<()> {
    let _guard = TerminalModeGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let controller = Controller::default();
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
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(2),
                Constraint::Length(1),
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

    let status = Paragraph::new(context.footer_body(&shell.status.render_body()))
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
        context.footer_body(&shell.status.render_body()),
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

    fn footer_body(&self, status: &str) -> String {
        format!(
            "{} | proj:{} | cwd:{} | prov:{} | model:{} | {}",
            status,
            compact_path_label(&self.project_root),
            compact_cwd_label(&self.project_root, &self.cwd),
            self.provider.as_deref().unwrap_or("none"),
            self.model.as_deref().unwrap_or("none"),
            compact_no_network_label()
        )
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

fn run_terminal_loop(
    terminal: &mut CrosstermTerminal,
    controller: &Controller,
    session: &mut Session,
    shell: &mut TuiShell,
) -> io::Result<()> {
    let mut input = TerminalInput::default();

    loop {
        shell.input.text = input.text().to_string();
        let context = TerminalShellContext::from_session(session);
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

fn handle_terminal_key(
    key: crossterm::event::KeyEvent,
    input: &mut TerminalInput,
    controller: &Controller,
    session: &mut Session,
    shell: &mut TuiShell,
) -> bool {
    if should_exit(key) {
        return true;
    }

    if is_approval_key(key) {
        shell.submit_approval(controller, session);
        return false;
    }

    if is_rejection_key(key) {
        shell.submit_rejection(controller, session);
        return false;
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
            if !submitted.trim().is_empty() {
                shell.submit_input(controller, session, &submitted);
            }
            false
        }
    }
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

fn is_approval_key(key: crossterm::event::KeyEvent) -> bool {
    key.code == KeyCode::F(5) && key.modifiers.is_empty()
}

fn is_rejection_key(key: crossterm::event::KeyEvent) -> bool {
    key.code == KeyCode::F(6) && key.modifiers.is_empty()
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
        default_shell_text, handle_scroll_key, handle_terminal_key, is_approval_key,
        is_rejection_key, render_tui_shell, should_exit, TerminalShellContext,
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
    fn default_terminal_shell_is_empty_and_no_network() {
        let text = default_shell_text();

        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("> "));
        assert!(text.contains("ready"));
        assert!(text.contains("prov:none"));
        assert!(text.contains("model:none"));
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
        assert!(text.contains("Press F5 to approve or F6 to reject"));
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

        assert!(text.contains("prov:local"));
        assert!(text.contains("model:model-a"));
        assert!(text.contains("Provider progress: response ready from local"));
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
    fn terminal_approval_and_rejection_keys_are_deliberate() {
        assert!(is_approval_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(5),
            crossterm::event::KeyModifiers::NONE
        )));
        assert!(is_rejection_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::F(6),
            crossterm::event::KeyModifiers::NONE
        )));
        assert!(!is_approval_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE
        )));
        assert!(!is_rejection_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('r'),
            crossterm::event::KeyModifiers::NONE
        )));
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
    fn terminal_f5_approves_pending_action_through_shell() {
        let controller = Controller::default();
        let root = temp_root("terminal-f5-approve");
        let target = root.join("approved.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();
        let mut input = TerminalInput::default();

        shell.submit_input(&controller, &mut session, "create file approved.py");
        assert!(!target.exists());

        let exited = handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::F(5),
                crossterm::event::KeyModifiers::NONE,
            ),
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
    fn terminal_f6_rejects_pending_action_through_shell() {
        let controller = Controller::default();
        let root = temp_root("terminal-f6-reject");
        let target = root.join("rejected.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();
        let mut input = TerminalInput::default();

        shell.submit_input(&controller, &mut session, "create file rejected.py");

        let exited = handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::F(6),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );

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
    fn terminal_approval_keys_show_no_pending_feedback() {
        let controller = Controller::default();
        let mut session = Session::new("session-1", "/repo", "/repo");
        let mut shell = TuiShell::new();
        let mut input = TerminalInput::default();

        handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::F(5),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );
        handle_terminal_key(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::F(6),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut input,
            &controller,
            &mut session,
            &mut shell,
        );

        let rendered = shell.render();
        assert!(rendered.contains("No proposed action is waiting for approval."));
        assert!(rendered.contains("No proposed action is waiting for rejection."));
        assert!(input.text().is_empty());
        assert!(session.actions().is_empty());
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("elgar-terminal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
