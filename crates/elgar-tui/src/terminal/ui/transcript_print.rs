//! Inline transcript printing helpers.
//!
//! This module owns stdout formatting for conversation transcript lines. It is
//! display-only and does not decide commands, approvals, or provider flow.

use std::io::{self, Write};

use crate::{
    panes::ConversationLineStyle,
    terminal::{
        ui::prompt::{drawable_width, non_empty_lines, terminal_width, wrap_words},
        ui::text::{conversation_print_blocks, pad_line, plain_block_lines},
        ANSI_EVENT, ANSI_MUTED, ANSI_RAW_DETAILS, ANSI_RESET, ANSI_TEXT, ANSI_USER_BLOCK,
    },
    TuiShell,
};

use super::code_syntax::write_code_line_ansi;
pub(crate) use super::code_syntax::CodeLineStyleState;

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
        ConversationLineStyle::Event => {
            print_spacer()?;
            print_event_block(line)
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

pub(crate) fn print_event_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in plain_block_lines(text, width) {
        writeln!(io::stdout(), "{ANSI_EVENT}{line}{ANSI_RESET}")?;
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
