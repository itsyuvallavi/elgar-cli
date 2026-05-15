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

use crate::TuiShell;

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
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(project_root, cwd);

    let result = run_terminal_loop(&mut terminal, &shell, &context);
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
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(area)
    };

    let conversation = Paragraph::new(shell.conversation.render_body())
        .wrap(Wrap { trim: false })
        .block(region_block("Conversation"));
    frame.render_widget(conversation, chunks[0]);

    let (input_index, status_index) = if shell.pending_action.panel.is_some() {
        let pending = Paragraph::new(shell.pending_action.render_body())
            .wrap(Wrap { trim: false })
            .block(region_block("Pending Action"));
        frame.render_widget(pending, chunks[1]);
        (2, 3)
    } else {
        (1, 2)
    };

    let input = Paragraph::new(shell.input.render_body()).block(region_block("Input"));
    frame.render_widget(input, chunks[input_index]);

    let status = Paragraph::new(context.footer_body(&shell.status.render_body()))
        .wrap(Wrap { trim: false })
        .block(region_block("Status"));
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
            "{} | project: {} | cwd: {} | provider: {} | model: {} | {}",
            status,
            self.project_root.display(),
            self.cwd.display(),
            self.provider.as_deref().unwrap_or("none active"),
            self.model.as_deref().unwrap_or("none"),
            default_no_network_line()
        )
    }
}

fn default_no_network_line() -> &'static str {
    "default no-network stub"
}

fn region_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
}

fn run_terminal_loop(
    terminal: &mut CrosstermTerminal,
    shell: &TuiShell,
    context: &TerminalShellContext,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| render_tui_shell(frame, shell, context))?;

        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press && should_exit(key) => break,
                _ => {}
            }
        }
    }

    Ok(())
}

fn should_exit(key: crossterm::event::KeyEvent) -> bool {
    matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c'))
}

struct TerminalModeGuard;

impl TerminalModeGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
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
    use elgar_core::{controller::Controller, session::Session};
    use ratatui::{backend::TestBackend, Terminal};

    use crate::TuiShell;

    use super::{default_shell_text, render_tui_shell, should_exit, TerminalShellContext};

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
        assert!(text.contains("provider: none active"));
        assert!(text.contains("model: none"));
        assert!(text.contains("default no-network stub"));
        assert!(!text.contains("lm-studio"));
    }

    #[test]
    fn terminal_layout_renders_default_shell_regions() {
        let shell = TuiShell::new();
        let context = TerminalShellContext::new("/repo", "/repo/crates");
        let text = draw_to_text(&shell, &context);

        assert!(text.contains("Conversation"));
        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("Input"));
        assert!(text.contains("> "));
        assert!(text.contains("Status"));
        assert!(text.contains("project: /repo"));
        assert!(text.contains("cwd: /repo/crates"));
        assert!(text.contains("provider: none active"));
        assert!(!text.contains("Pending Action"));
    }

    #[test]
    fn terminal_layout_renders_pending_action_only_when_present() {
        let controller = Controller::default();
        let mut session = Session::new("session-1", "/repo", "/repo");
        let mut shell = TuiShell::new();

        let result = controller.turn(&mut session, "create file hello.py");
        shell.consume_events(&result.events);

        let text = draw_to_text(&shell, &TerminalShellContext::from_session(&session));

        assert!(text.contains("Conversation"));
        assert!(text.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(text.contains("Pending Action"));
        assert!(text.contains("Action: action-1 WriteFile"));
        assert!(text.contains("State: waiting for approval"));
        assert!(text.contains("Input"));
        assert!(text.contains("Status"));
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

        assert!(text.contains("provider: local"));
        assert!(text.contains("model: model-a"));
        assert!(text.contains("Response from local"));
    }

    #[test]
    fn terminal_shell_exit_keys_are_minimal() {
        assert!(should_exit(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE
        )));
        assert!(should_exit(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('q'),
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
    }
}
