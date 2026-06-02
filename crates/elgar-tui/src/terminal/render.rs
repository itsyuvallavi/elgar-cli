use std::io::{self, Write};

use ratatui::{
    layout::{Constraint, Direction, Layout},
    text::{Line, Span, Text},
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

use super::{
    ANSI_CODE_BODY, ANSI_CODE_BORDER, ANSI_CODE_COMMENT, ANSI_CODE_HEADER, ANSI_CODE_HINT,
    ANSI_CODE_KEY, ANSI_CODE_LITERAL, ANSI_CODE_NUMBER, ANSI_CODE_STRING, ANSI_MUTED,
    ANSI_RAW_DETAILS, ANSI_RESET, ANSI_TEXT, ANSI_TOOL_BLOCK, ANSI_TOOL_ERROR, ANSI_TOOL_SUCCESS,
    ANSI_TOOL_WARNING, ANSI_USER_BLOCK,
};

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
    let mut code_state = CodeLineStyleState::default();
    for line in plain_block_lines(text, width) {
        write_transcript_line_ansi(&line, "\n", &mut code_state)?;
    }
    io::stdout().flush()
}

pub(super) fn print_model_block(text: &str) -> io::Result<()> {
    print_plain_block(text)
}

pub(super) fn print_state_block(text: &str) -> io::Result<()> {
    writeln!(io::stdout(), "{ANSI_MUTED}state{ANSI_RESET}")?;
    print_plain_block(text)
}

pub(super) fn print_details_block(text: &str) -> io::Result<()> {
    let width = drawable_width(terminal_width());
    for line in plain_block_lines(text, width) {
        writeln!(io::stdout(), "{ANSI_RAW_DETAILS}{line}{ANSI_RESET}")?;
    }
    io::stdout().flush()
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
        let padded = pad_line(&line, width);
        writeln!(
            io::stdout(),
            "{}{padded}{ANSI_RESET}",
            tool_line_ansi(&line)
        )?;
    }
    io::stdout().flush()
}

pub(super) fn write_transcript_line_ansi(
    line: &str,
    newline: &str,
    code_state: &mut CodeLineStyleState,
) -> io::Result<()> {
    if write_code_line_ansi(line, newline, code_state)? {
        return Ok(());
    }

    code_state.language = None;
    write!(
        io::stdout(),
        "{}{line}{ANSI_RESET}{newline}",
        transcript_output_ansi()
    )
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
                code_state.language = None;
                Line::styled(line, theme::raw_details())
            }
            ConversationLineStyle::Tool => {
                code_state.language = None;
                Line::styled(pad_line(&line, width), tool_line_style(&line))
            }
        });
        previous_style = Some(style);
    }

    Text::from(lines)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct CodeLineStyleState {
    language: Option<String>,
}

fn styled_transcript_line(
    line: &str,
    fallback: ratatui::style::Style,
    code_state: &mut CodeLineStyleState,
) -> Line<'static> {
    styled_code_line(line, code_state).unwrap_or_else(|| {
        code_state.language = None;
        Line::styled(line.to_string(), fallback)
    })
}

fn styled_code_line(line: &str, code_state: &mut CodeLineStyleState) -> Option<Line<'static>> {
    if let Some((border_prefix, header, border_suffix)) = split_code_header_line(line) {
        code_state.language = code_header_language(&header);
        return Some(Line::from(vec![
            Span::styled(border_prefix, theme::code_border()),
            Span::styled(header, theme::code_header()),
            Span::styled(border_suffix, theme::code_border()),
        ]));
    }

    if is_code_bottom_line(line) {
        code_state.language = None;
        return Some(Line::styled(line.to_string(), theme::code_border()));
    }

    if let Some((border_prefix, body, border_suffix)) = split_code_body_line(line) {
        let mut spans = Vec::new();
        spans.push(Span::styled(border_prefix, theme::code_border()));
        if is_code_hint_body(&body) {
            spans.push(Span::styled(body, theme::code_hint()));
        } else {
            spans.extend(
                code_syntax_segments(&body, code_state.language.as_deref())
                    .into_iter()
                    .map(|segment| Span::styled(segment.text, segment.style.ratatui_style())),
            );
        }
        spans.push(Span::styled(border_suffix, theme::code_border()));
        return Some(Line::from(spans));
    }

    None
}

fn write_code_line_ansi(
    line: &str,
    newline: &str,
    code_state: &mut CodeLineStyleState,
) -> io::Result<bool> {
    if let Some((border_prefix, header, border_suffix)) = split_code_header_line(line) {
        code_state.language = code_header_language(&header);
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{border_prefix}{ANSI_CODE_HEADER}{header}{ANSI_CODE_BORDER}{border_suffix}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    if is_code_bottom_line(line) {
        code_state.language = None;
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{line}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    if let Some((border_prefix, body, border_suffix)) = split_code_body_line(line) {
        write!(io::stdout(), "{ANSI_CODE_BORDER}{border_prefix}")?;
        if is_code_hint_body(&body) {
            write!(io::stdout(), "{ANSI_CODE_HINT}{body}")?;
        } else {
            for segment in code_syntax_segments(&body, code_state.language.as_deref()) {
                write!(io::stdout(), "{}{}", segment.style.ansi(), segment.text)?;
            }
        }
        write!(
            io::stdout(),
            "{ANSI_CODE_BORDER}{border_suffix}{ANSI_RESET}{newline}"
        )?;
        return Ok(true);
    }

    Ok(false)
}

fn code_header_language(header: &str) -> Option<String> {
    let language = header.strip_prefix("code (")?.split_once(')')?.0.trim();
    if language.is_empty() {
        None
    } else {
        Some(language.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeSyntaxSegment {
    text: String,
    style: CodeTokenStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeTokenStyle {
    Body,
    Key,
    String,
    Number,
    Literal,
    Comment,
}

impl CodeTokenStyle {
    fn ratatui_style(self) -> ratatui::style::Style {
        match self {
            CodeTokenStyle::Body => theme::code_body(),
            CodeTokenStyle::Key => theme::code_key(),
            CodeTokenStyle::String => theme::code_string(),
            CodeTokenStyle::Number => theme::code_number(),
            CodeTokenStyle::Literal => theme::code_literal(),
            CodeTokenStyle::Comment => theme::code_comment(),
        }
    }

    fn ansi(self) -> &'static str {
        match self {
            CodeTokenStyle::Body => ANSI_CODE_BODY,
            CodeTokenStyle::Key => ANSI_CODE_KEY,
            CodeTokenStyle::String => ANSI_CODE_STRING,
            CodeTokenStyle::Number => ANSI_CODE_NUMBER,
            CodeTokenStyle::Literal => ANSI_CODE_LITERAL,
            CodeTokenStyle::Comment => ANSI_CODE_COMMENT,
        }
    }
}

fn code_syntax_segments(body: &str, language: Option<&str>) -> Vec<CodeSyntaxSegment> {
    let Some(language) = normalized_code_language(language) else {
        return vec![CodeSyntaxSegment {
            text: body.to_string(),
            style: CodeTokenStyle::Body,
        }];
    };

    let content = body.trim_end_matches(char::is_whitespace);
    let trailing = &body[content.len()..];
    let mut segments = syntax_segments_for_content(content, &language);
    push_code_segment(&mut segments, trailing, CodeTokenStyle::Body);
    segments
}

fn normalized_code_language(language: Option<&str>) -> Option<String> {
    let normalized = language?
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    let normalized = match normalized.as_str() {
        "bash" | "sh" | "shell" | "zsh" => "bash",
        "json" => "json",
        "javascript" | "js" | "jsx" => "javascript",
        "python" | "py" => "python",
        "rust" | "rs" => "rust",
        "toml" => "toml",
        "typescript" | "ts" | "tsx" => "typescript",
        "yaml" | "yml" => "yaml",
        "plain" | "plaintext" | "text" | "txt" => return None,
        _ => return None,
    };
    Some(normalized.to_string())
}

fn syntax_segments_for_content(content: &str, language: &str) -> Vec<CodeSyntaxSegment> {
    if let Some(comment_start) = line_comment_start(content, language) {
        let mut segments = Vec::new();
        push_code_segment(
            &mut segments,
            &content[..comment_start],
            CodeTokenStyle::Body,
        );
        push_code_segment(
            &mut segments,
            &content[comment_start..],
            CodeTokenStyle::Comment,
        );
        return segments;
    }

    if let Some((prefix, key, separator, rest)) = split_config_key(content, language) {
        let mut segments = Vec::new();
        push_code_segment(&mut segments, prefix, CodeTokenStyle::Body);
        push_code_segment(&mut segments, key, CodeTokenStyle::Key);
        push_code_segment(&mut segments, separator, CodeTokenStyle::Body);
        segments.extend(scan_code_tokens(rest, language));
        return segments;
    }

    scan_code_tokens(content, language)
}

fn line_comment_start(content: &str, language: &str) -> Option<usize> {
    let marker = comment_marker(language)?;
    let trimmed = content.trim_start();
    if trimmed.starts_with(marker) {
        Some(content.len() - trimmed.len())
    } else {
        None
    }
}

fn split_config_key<'a>(
    content: &'a str,
    language: &str,
) -> Option<(&'a str, &'a str, &'a str, &'a str)> {
    let value = content.trim_start();
    let prefix_len = content.len() - value.len();
    let prefix = &content[..prefix_len];

    if language == "json" {
        let key_end = closing_quote_index(value, '"')?;
        let after_key = &value[(key_end + 1)..];
        let colon = after_key.find(':')?;
        let after_colon = &after_key[(colon + 1)..];
        let rest = after_colon.trim_start();
        let separator_len = colon + 1 + after_colon.len() - rest.len();
        return Some((
            prefix,
            &value[..=key_end],
            &after_key[..separator_len],
            rest,
        ));
    }

    let separator = match language {
        "toml" => '=',
        "yaml" => ':',
        _ => return None,
    };
    if value.starts_with('[') {
        return None;
    }

    let key_len = value
        .char_indices()
        .take_while(|(_, character)| is_config_key_character(*character))
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let key = &value[..key_len];
    let after_key = &value[key_len..];
    let after_key_trimmed = after_key.trim_start();
    if !after_key_trimmed.starts_with(separator) {
        return None;
    }

    let before_separator_len = after_key.len() - after_key_trimmed.len();
    let after_separator = &after_key_trimmed[separator.len_utf8()..];
    let rest = after_separator.trim_start();
    let separator_len =
        before_separator_len + separator.len_utf8() + after_separator.len() - rest.len();
    Some((prefix, key, &after_key[..separator_len], rest))
}

fn scan_code_tokens(content: &str, language: &str) -> Vec<CodeSyntaxSegment> {
    let mut segments = Vec::new();
    let mut index = 0;
    while index < content.len() {
        let rest = &content[index..];
        if starts_inline_comment(content, index, language) {
            push_code_segment(&mut segments, rest, CodeTokenStyle::Comment);
            break;
        }

        let character = rest.chars().next().expect("non-empty rest");
        if matches!(character, '"' | '\'') {
            let length = closing_quote_index(rest, character)
                .map(|quote_index| quote_index + character.len_utf8())
                .unwrap_or(rest.len());
            push_code_segment(&mut segments, &rest[..length], CodeTokenStyle::String);
            index += length;
            continue;
        }

        if let Some(length) = number_literal_len(rest) {
            push_code_segment(&mut segments, &rest[..length], CodeTokenStyle::Number);
            index += length;
            continue;
        }

        if is_identifier_start(character) {
            let length = identifier_len(rest);
            let identifier = &rest[..length];
            let style = if is_literal_identifier(identifier) {
                CodeTokenStyle::Literal
            } else {
                CodeTokenStyle::Body
            };
            push_code_segment(&mut segments, identifier, style);
            index += length;
            continue;
        }

        push_code_segment(
            &mut segments,
            &rest[..character.len_utf8()],
            CodeTokenStyle::Body,
        );
        index += character.len_utf8();
    }
    segments
}

fn push_code_segment(segments: &mut Vec<CodeSyntaxSegment>, text: &str, style: CodeTokenStyle) {
    if text.is_empty() {
        return;
    }
    if let Some(previous) = segments.last_mut().filter(|segment| segment.style == style) {
        previous.text.push_str(text);
    } else {
        segments.push(CodeSyntaxSegment {
            text: text.to_string(),
            style,
        });
    }
}

fn comment_marker(language: &str) -> Option<&'static str> {
    match language {
        "bash" | "python" | "toml" | "yaml" => Some("#"),
        "javascript" | "rust" | "typescript" => Some("//"),
        _ => None,
    }
}

fn starts_inline_comment(content: &str, index: usize, language: &str) -> bool {
    let Some(marker) = comment_marker(language) else {
        return false;
    };
    if !content[index..].starts_with(marker) {
        return false;
    }
    index == 0
        || content[..index]
            .chars()
            .last()
            .is_some_and(|character| character.is_whitespace())
}

fn closing_quote_index(value: &str, quote: char) -> Option<usize> {
    if !value.starts_with(quote) {
        return None;
    }

    let mut escaped = false;
    for (index, character) in value.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == quote {
            return Some(index);
        }
    }
    None
}

fn number_literal_len(value: &str) -> Option<usize> {
    let mut chars = value.char_indices().peekable();
    if chars.peek().is_some_and(|(_, character)| *character == '-') {
        chars.next();
    }
    if !chars
        .peek()
        .is_some_and(|(_, character)| character.is_ascii_digit())
    {
        return None;
    }

    let mut end = 0;
    for (index, character) in chars {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '+' | '-') {
            end = index + character.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn identifier_len(value: &str) -> usize {
    value
        .char_indices()
        .take_while(|(_, character)| is_identifier_character(*character))
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0)
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn is_literal_identifier(identifier: &str) -> bool {
    matches!(
        identifier,
        "Err" | "False" | "None" | "Ok" | "Some" | "True" | "false" | "null" | "true" | "undefined"
    )
}

fn is_config_key_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
}

fn split_code_header_line(line: &str) -> Option<(String, String, String)> {
    let body = line.strip_prefix(" ╭─ ")?.strip_suffix('╮')?;
    if !body.starts_with("code") {
        return None;
    }

    let header = body.trim_end_matches('─').trim_end();
    let suffix = &body[header.len()..];
    Some((" ╭─ ".to_string(), header.to_string(), format!("{suffix}╮")))
}

fn split_code_body_line(line: &str) -> Option<(String, String, String)> {
    let body = line.strip_prefix(" │ ")?.strip_suffix(" │")?;
    Some((" │ ".to_string(), body.to_string(), " │".to_string()))
}

fn is_code_bottom_line(line: &str) -> bool {
    line.starts_with(" ╰") && line.ends_with('╯') && line.chars().all(is_code_border_character)
}

fn is_code_border_character(character: char) -> bool {
    matches!(character, ' ' | '╰' | '╯' | '─')
}

fn is_code_hint_body(body: &str) -> bool {
    body.trim_start().starts_with("... ") && body.contains(" hidden; ")
}

fn tool_line_style(line: &str) -> ratatui::style::Style {
    match tool_line_tone(line) {
        ToolLineTone::Success => theme::tool_success(),
        ToolLineTone::Warning => theme::tool_warning(),
        ToolLineTone::Error => theme::tool_error(),
        ToolLineTone::Neutral => theme::tool_neutral(),
    }
}

fn tool_line_ansi(line: &str) -> &'static str {
    match tool_line_tone(line) {
        ToolLineTone::Success => ANSI_TOOL_SUCCESS,
        ToolLineTone::Warning => ANSI_TOOL_WARNING,
        ToolLineTone::Error => ANSI_TOOL_ERROR,
        ToolLineTone::Neutral => ANSI_TOOL_BLOCK,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolLineTone {
    Success,
    Warning,
    Error,
    Neutral,
}

fn tool_line_tone(line: &str) -> ToolLineTone {
    let line = line.trim();
    if line.contains("failed")
        || line.contains("timed out")
        || line.starts_with("Tool call incomplete:")
    {
        ToolLineTone::Error
    } else if line.contains("truncated")
        || line.starts_with("Outside project:")
        || line.starts_with("stderr hidden")
    {
        ToolLineTone::Warning
    } else if line.starts_with("Created project:")
        || line.starts_with("Verified:")
        || line.starts_with("shell command finished")
        || line.starts_with("listed files")
        || line.starts_with("[ok]")
    {
        ToolLineTone::Success
    } else {
        ToolLineTone::Neutral
    }
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
