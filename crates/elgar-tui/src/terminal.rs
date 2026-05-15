use std::{
    io::{self, Stdout},
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

type CrosstermTerminal = Terminal<CrosstermBackend<Stdout>>;

pub fn run_terminal_shell() -> io::Result<()> {
    let _guard = TerminalModeGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;

    let result = run_terminal_loop(&mut terminal);
    let cursor_result = terminal.show_cursor();

    result.and(cursor_result)
}

pub fn render_default_terminal_shell(frame: &mut Frame<'_>) {
    let area = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(area);

    let conversation = Paragraph::new(default_shell_lines()).block(
        Block::default()
            .title("Elgar")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(conversation, chunks[0]);

    let footer = Paragraph::new("ready | default stub mode | q, Esc, or Ctrl-C exits").block(
        Block::default()
            .title("Status")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, chunks[1]);
}

pub fn default_shell_text() -> &'static str {
    "Elgar terminal shell\n(empty conversation)\nNo provider or network access is active."
}

fn default_shell_lines() -> Vec<Line<'static>> {
    default_shell_text().lines().map(Line::from).collect()
}

fn run_terminal_loop(terminal: &mut CrosstermTerminal) -> io::Result<()> {
    loop {
        terminal.draw(render_default_terminal_shell)?;

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
    use super::{default_shell_text, should_exit};

    #[test]
    fn default_terminal_shell_is_empty_and_no_network() {
        let text = default_shell_text();

        assert!(text.contains("Elgar terminal shell"));
        assert!(text.contains("(empty conversation)"));
        assert!(text.contains("No provider or network access is active."));
        assert!(!text.contains("lm-studio"));
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
