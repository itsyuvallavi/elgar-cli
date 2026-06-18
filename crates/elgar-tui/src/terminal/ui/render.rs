//! Ratatui frame rendering helpers.
//!
//! This is the lower-level renderer for framed terminal layouts. The current
//! inline prompt path mainly uses direct stdout rendering.

use std::io::{self, Write};

use ratatui::{
    layout::{Constraint, Direction, Layout},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::{
    panes::{ConversationLineStyle, ConversationPane},
    startup::StartupBlock,
    terminal::{
        display_context::{default_no_network_line, TerminalShellContext},
        ui::prompt::{drawable_width, non_empty_lines, terminal_width, wrap_words},
        ui::text::{conversation_print_blocks, pad_line, plain_block_lines},
        ANSI_MUTED, ANSI_RAW_DETAILS, ANSI_RESET, ANSI_TEXT, ANSI_USER_BLOCK,
    },
    theme, TuiShell,
};

pub(crate) use super::code_syntax::CodeLineStyleState;
use super::code_syntax::{styled_transcript_line, write_code_line_ansi};

pub(crate) fn transcript_output_ansi() -> &'static str {
    ANSI_TEXT
}

pub(crate) fn print_new_conversation_lines(
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

pub(crate) fn print_conversation_line(line: &str, style: ConversationLineStyle) -> io::Result<()> {
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
        ConversationLineStyle::Thinking | ConversationLineStyle::Metrics => {
            print_spacer()?;
            writeln!(io::stdout(), "{ANSI_MUTED}{line}{ANSI_RESET}")
        }
        ConversationLineStyle::Plain => {
            print_spacer()?;
            print_plain_block(line)
        }
        ConversationLineStyle::Details => {
            print_spacer()?;
            print_details_block(line)
        }
        ConversationLineStyle::VerifiedState => {
            print_spacer()?;
            print_state_block(line)
        }
        ConversationLineStyle::Model => {
            print_spacer()?;
            print_model_block(line)
        }
    }
}

pub(crate) fn print_spacer() -> io::Result<()> {
    writeln!(io::stdout())
}

pub(crate) fn print_user_block(input: &str) -> io::Result<()> {
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

pub(crate) fn print_plain_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    let mut code_state = CodeLineStyleState::default();
    for line in plain_block_lines(text, width) {
        write_transcript_line_ansi(&line, "\n", &mut code_state)?;
    }
    io::stdout().flush()
}

pub(crate) fn print_model_block(text: &str) -> io::Result<()> {
    print_plain_block(text)
}

pub(crate) fn print_state_block(text: &str) -> io::Result<()> {
    writeln!(io::stdout(), "{ANSI_MUTED}state{ANSI_RESET}")?;
    print_plain_block(text)
}

pub(crate) fn print_details_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in plain_block_lines(text, width) {
        writeln!(io::stdout(), "{ANSI_RAW_DETAILS}{line}{ANSI_RESET}")?;
    }
    io::stdout().flush()
}

pub(crate) fn print_and_record_local(
    shell: &mut TuiShell,
    text: impl Into<String>,
) -> io::Result<()> {
    let text = text.into();
    shell.push_local_message(text.clone());
    print_plain_block(&text)
}

pub(crate) fn write_transcript_line_ansi(
    line: &str,
    newline: &str,
    code_state: &mut CodeLineStyleState,
) -> io::Result<()> {
    if write_code_line_ansi(line, newline, code_state)? {
        return Ok(());
    }

    code_state.reset();
    write!(
        io::stdout(),
        "{}{line}{ANSI_RESET}{newline}",
        transcript_output_ansi()
    )
}

/// Render the default empty shell frame.
pub fn render_default_terminal_shell(frame: &mut Frame<'_>) {
    let shell = TuiShell::new();
    let context = TerminalShellContext::new(".", ".").with_provider("stub-provider", None);
    render_tui_shell(frame, &shell, &context);
}

/// Render the current shell state into a ratatui frame.
pub fn render_tui_shell(frame: &mut Frame<'_>, shell: &TuiShell, context: &TerminalShellContext) {
    let area = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(2),
            Constraint::Length(2),
        ])
        .split(area);

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

    let input_index = 1;
    let status_index = 2;

    let input = Paragraph::new(shell.input.render_body())
        .style(theme::user_input_block())
        .block(divider_block(""));
    frame.render_widget(input, chunks[input_index]);

    let status =
        Paragraph::new(context.footer_body_for_width(usize::from(chunks[status_index].width)))
            .style(context.footer_style())
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

pub(super) fn render_terminal_conversation(
    shell: &TuiShell,
    context: &TerminalShellContext,
) -> String {
    let startup = render_terminal_startup(context);
    format!("{}\n\n{}", startup, shell.conversation.render_body())
}

pub(crate) fn render_terminal_startup(context: &TerminalShellContext) -> String {
    let startup = StartupBlock::from_context_accounting_with_mcp(
        context.provider.clone(),
        context.model.clone(),
        &context.context_accounting,
        context.mcp_status.clone(),
    );
    startup.render()
}

pub(super) fn terminal_conversation_line_count(
    startup: &str,
    conversation: &ConversationPane,
) -> usize {
    let lines = conversation.render_lines_with_styles();
    startup.lines().count() + 2 + lines.len()
}

pub(crate) fn style_terminal_conversation(
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

    let mut previous_style = None;
    let mut code_state = CodeLineStyleState::default();
    for (line, style) in conversation.render_lines_with_styles() {
        if style == ConversationLineStyle::VerifiedState
            && previous_style != Some(ConversationLineStyle::VerifiedState)
        {
            lines.push(Line::styled("state", theme::muted()));
        }
        lines.push(match style {
            ConversationLineStyle::User => {
                let visible = line.strip_prefix("> ").unwrap_or(&line);
                Line::styled(pad_line(visible, width), theme::user_input_block())
            }
            ConversationLineStyle::Model => {
                styled_transcript_line(&line, theme::model_output(), &mut code_state)
            }
            ConversationLineStyle::VerifiedState => {
                styled_transcript_line(&line, theme::model_output(), &mut code_state)
            }
            ConversationLineStyle::Loading => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Thinking => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Metrics => Line::styled(line, theme::muted()),
            ConversationLineStyle::Plain => {
                styled_transcript_line(&line, theme::model_output(), &mut code_state)
            }
            ConversationLineStyle::Details => {
                code_state.reset();
                Line::styled(line, theme::raw_details())
            }
        });
        previous_style = Some(style);
    }

    Text::from(lines)
}

pub(super) fn divider_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::TOP)
        .title_style(theme::accent())
        .border_style(theme::muted())
}
