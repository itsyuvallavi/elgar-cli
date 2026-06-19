//! Ratatui frame rendering helpers.
//!
//! This is the lower-level renderer for framed terminal layouts. The current
//! inline prompt path mainly uses direct stdout rendering.

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
        ui::text::pad_line,
    },
    theme, TuiShell,
};

use super::code_syntax::styled_transcript_line;
pub(crate) use super::code_syntax::CodeLineStyleState;
pub(crate) use super::transcript_print::{
    print_and_record_local, print_conversation_line, print_new_conversation_lines,
    print_plain_block, print_spacer, print_user_block, write_transcript_line_ansi,
};

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
            ConversationLineStyle::Event => Line::styled(line, theme::event()),
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
