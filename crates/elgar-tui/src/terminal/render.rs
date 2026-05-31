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
    terminal::context::{default_no_network_line, TerminalShellContext},
    terminal::prompt::{drawable_width, non_empty_lines, terminal_width, wrap_words},
    terminal::text::{conversation_print_blocks, pad_line, plain_block_lines},
    theme, TuiShell,
};

use super::{ANSI_MUTED, ANSI_RESET, ANSI_TEXT, ANSI_TOOL_BLOCK, ANSI_USER_BLOCK};

pub(super) fn transcript_output_ansi() -> &'static str {
    ANSI_TEXT
}

pub(super) fn print_new_conversation_lines(
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

pub(super) fn print_conversation_line(line: &str, style: ConversationLineStyle) -> io::Result<()> {
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
        ConversationLineStyle::Model => {
            print_spacer()?;
            print_model_block(line)
        }
        ConversationLineStyle::Tool => {
            print_spacer()?;
            print_tool_block(line)
        }
    }
}

pub(super) fn print_spacer() -> io::Result<()> {
    writeln!(io::stdout())
}

pub(super) fn print_user_block(input: &str) -> io::Result<()> {
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

pub(super) fn print_plain_block(text: &str) -> io::Result<()> {
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

pub(super) fn print_model_block(text: &str) -> io::Result<()> {
    writeln!(io::stdout(), "{ANSI_MUTED}model{ANSI_RESET}")?;
    print_plain_block(text)
}

pub(super) fn print_and_record_local(
    shell: &mut TuiShell,
    text: impl Into<String>,
) -> io::Result<()> {
    let text = text.into();
    shell.push_local_message(text.clone());
    print_plain_block(&text)
}

pub(super) fn print_tool_block(text: &str) -> io::Result<()> {
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

pub(super) fn render_terminal_startup(context: &TerminalShellContext) -> String {
    let startup = StartupBlock::from_context_accounting(
        context.provider.clone(),
        context.model.clone(),
        context.policy_mode,
        &context.context_accounting,
    );
    startup.render()
}

pub(super) fn terminal_conversation_line_count(
    startup: &str,
    conversation: &ConversationPane,
) -> usize {
    let lines = conversation.render_lines_with_styles();
    startup.lines().count() + 2 + lines.len() + model_block_count(&lines)
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
    for (line, style) in conversation.render_lines_with_styles() {
        if style == ConversationLineStyle::Model
            && previous_style != Some(ConversationLineStyle::Model)
        {
            lines.push(Line::styled("model", theme::muted()));
        }
        lines.push(match style {
            ConversationLineStyle::User => {
                let visible = line.strip_prefix("> ").unwrap_or(&line);
                Line::styled(pad_line(visible, width), theme::user_input_block())
            }
            ConversationLineStyle::Model => Line::styled(line, theme::model_output()),
            ConversationLineStyle::Loading => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Thinking => Line::styled(line, theme::thinking()),
            ConversationLineStyle::Metrics => Line::styled(line, theme::muted()),
            ConversationLineStyle::Plain => Line::styled(line, theme::model_output()),
            ConversationLineStyle::Tool => {
                Line::styled(pad_line(&line, width), theme::tool_output())
            }
        });
        previous_style = Some(style);
    }

    Text::from(lines)
}

fn model_block_count(lines: &[(String, ConversationLineStyle)]) -> usize {
    let mut count = 0;
    let mut previous_style = None;
    for (_line, style) in lines {
        if *style == ConversationLineStyle::Model
            && previous_style != Some(ConversationLineStyle::Model)
        {
            count += 1;
        }
        previous_style = Some(*style);
    }
    count
}

pub(super) fn divider_block(title: &'static str) -> Block<'static> {
    Block::default()
        .title(title)
        .borders(Borders::TOP)
        .title_style(theme::accent())
        .border_style(theme::muted())
}

#[cfg(test)]
pub(crate) fn status_style(status: &str) -> ratatui::style::Style {
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
