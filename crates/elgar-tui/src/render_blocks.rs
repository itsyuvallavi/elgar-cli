pub(crate) const CODE_BLOCK_COLLAPSE_LINE_THRESHOLD: usize = 80;
pub(crate) const CODE_BLOCK_VISIBLE_LINE_LIMIT: usize = 40;
const CODE_BLOCK_COLLAPSE_CHAR_THRESHOLD: usize = 4_000;
const CODE_BOX_MIN_CONTENT_WIDTH: usize = 64;
const CODE_BOX_MAX_CONTENT_WIDTH: usize = 72;
const CODE_WRAP_CONTINUATION_PREFIX: &str = "  ↳ ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeBlockRender {
    pub(crate) lines: Vec<String>,
    pub(crate) collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeBlockInput {
    pub(crate) info: String,
    pub(crate) lines: Vec<String>,
}

impl CodeBlockInput {
    pub(crate) fn new(info: impl Into<String>, lines: Vec<String>) -> Self {
        Self {
            info: info.into(),
            lines,
        }
    }
}

pub(crate) fn render_code_block(input: CodeBlockInput) -> CodeBlockRender {
    let info = CodeFenceInfo::parse(&input.info);
    let display_lines = compact_display_lines(trim_code_edges(&input.lines));
    let line_count = display_lines.len();
    let collapsed = code_block_would_collapse(&display_lines);
    let shown_line_count = if collapsed {
        CODE_BLOCK_VISIBLE_LINE_LIMIT.min(line_count)
    } else {
        line_count
    };

    let header = render_code_header(&info, line_count, collapsed.then_some(shown_line_count));
    let mut body_lines = display_lines
        .iter()
        .take(shown_line_count)
        .cloned()
        .collect::<Vec<_>>();

    if collapsed {
        let hidden = line_count.saturating_sub(shown_line_count);
        body_lines.push(format!(
            "... {hidden} lines hidden; use /details last or /copy raw"
        ));
    }

    let lines = render_boxed_code_block(&header, &body_lines);

    CodeBlockRender { lines, collapsed }
}

pub(crate) fn code_block_would_collapse(lines: &[String]) -> bool {
    let display_lines = compact_display_lines(trim_code_edges(lines));
    display_lines.len() > CODE_BLOCK_COLLAPSE_LINE_THRESHOLD
        || display_lines
            .iter()
            .map(|line| line.len().saturating_add(1))
            .sum::<usize>()
            > CODE_BLOCK_COLLAPSE_CHAR_THRESHOLD
}

fn trim_code_edges(lines: &[String]) -> &[String] {
    let Some(start) = lines.iter().position(|line| !line.trim().is_empty()) else {
        return &[];
    };
    let end = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);
    &lines[start..end]
}

fn compact_display_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .cloned()
        .collect()
}

fn render_code_header(
    info: &CodeFenceInfo,
    line_count: usize,
    collapsed_shown_lines: Option<usize>,
) -> String {
    let mut parts = vec![match info.language.as_deref() {
        Some(language) => format!("code ({language})"),
        None => "code".to_string(),
    }];
    if let Some(label) = info.label.as_deref() {
        parts.push(label.to_string());
    }
    parts.push(line_count_label(line_count));
    if let Some(shown_lines) = collapsed_shown_lines {
        parts.push(format!("collapsed, showing {shown_lines}"));
    }
    parts.join(" · ")
}

fn line_count_label(line_count: usize) -> String {
    if line_count == 1 {
        "1 line".to_string()
    } else {
        format!("{line_count} lines")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeFenceInfo {
    language: Option<String>,
    label: Option<String>,
}

impl CodeFenceInfo {
    fn parse(info: &str) -> Self {
        let info = info.trim();
        if info.is_empty() {
            return Self {
                language: None,
                label: None,
            };
        }

        let mut parts = info.split_whitespace();
        let Some(first) = parts.next() else {
            return Self {
                language: None,
                label: None,
            };
        };
        let rest = parts.collect::<Vec<_>>().join(" ");

        if rest.is_empty() && looks_like_path(first) {
            return Self {
                language: extension_language(first),
                label: Some(first.to_string()),
            };
        }

        if is_language_token(first) {
            return Self {
                language: Some(first.to_string()),
                label: (!rest.is_empty()).then_some(rest),
            };
        }

        Self {
            language: None,
            label: Some(info.to_string()),
        }
    }
}

fn is_language_token(token: &str) -> bool {
    !token.is_empty()
        && token.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '+' | '#')
        })
}

fn looks_like_path(token: &str) -> bool {
    token.contains('/') || token.contains('\\') || token.contains('.')
}

fn extension_language(path: &str) -> Option<String> {
    let extension = path.rsplit_once('.')?.1;
    let language = extension
        .chars()
        .take_while(|character| character.is_ascii_alphanumeric())
        .collect::<String>();
    (!language.is_empty()).then_some(language)
}

fn render_boxed_code_block(header: &str, body_lines: &[String]) -> Vec<String> {
    let content_width = code_box_content_width(header, body_lines);
    let header = truncate_to_width(header, content_width);
    let mut lines = vec![code_box_top_line(&header, content_width)];

    for line in body_lines {
        for segment in split_to_width(line, content_width) {
            lines.push(code_box_body_line(&segment, content_width));
        }
    }

    lines.push(code_box_bottom_line(content_width));
    lines
}

fn code_box_content_width(header: &str, body_lines: &[String]) -> usize {
    body_lines
        .iter()
        .map(|line| line.chars().count())
        .chain(std::iter::once(header.chars().count()))
        .max()
        .unwrap_or(0)
        .clamp(CODE_BOX_MIN_CONTENT_WIDTH, CODE_BOX_MAX_CONTENT_WIDTH)
}

fn code_box_top_line(header: &str, content_width: usize) -> String {
    let prefix = format!("─ {header} ");
    let rule_width = content_width + 3;
    let fill = "─".repeat(rule_width.saturating_sub(prefix.chars().count()));
    format!(" ╭{prefix}{fill}╮")
}

fn code_box_body_line(line: &str, content_width: usize) -> String {
    format!(
        " │ {}{} │",
        line,
        " ".repeat(content_width.saturating_sub(line.chars().count()))
    )
}

fn code_box_bottom_line(content_width: usize) -> String {
    format!(" ╰{}╯", "─".repeat(content_width + 3))
}

fn split_to_width(line: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    if line.is_empty() {
        return vec![String::new()];
    }

    if line.chars().count() <= width {
        return vec![line.to_string()];
    }

    let continuation_width = width
        .saturating_sub(CODE_WRAP_CONTINUATION_PREFIX.chars().count())
        .max(1);
    let mut segments = Vec::new();
    let mut rest = line;
    let mut is_continuation = false;

    while !rest.is_empty() {
        let available_width = if is_continuation {
            continuation_width
        } else {
            width
        };
        let (chunk, next) = split_code_display_chunk(rest, available_width);
        let chunk = chunk.trim_end();
        if is_continuation {
            segments.push(format!("{CODE_WRAP_CONTINUATION_PREFIX}{chunk}"));
        } else {
            segments.push(chunk.to_string());
        }
        rest = next.trim_start_matches(char::is_whitespace);
        is_continuation = true;
    }

    segments
}

fn split_code_display_chunk(line: &str, width: usize) -> (&str, &str) {
    if line.chars().count() <= width {
        return (line, "");
    }

    let byte_limit = byte_index_after_chars(line, width);
    if let Some((break_index, _character)) = line[..byte_limit]
        .char_indices()
        .rev()
        .find(|(index, character)| *index > 0 && character.is_whitespace())
    {
        return (&line[..break_index], &line[break_index..]);
    }

    (&line[..byte_limit], &line[byte_limit..])
}

fn byte_index_after_chars(text: &str, max_chars: usize) -> usize {
    text.char_indices()
        .nth(max_chars)
        .map(|(index, _character)| index)
        .unwrap_or(text.len())
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::{render_code_block, CodeBlockInput};

    #[test]
    fn renders_code_block_metadata() {
        let rendered = render_code_block(CodeBlockInput::new(
            "tsx app/page.tsx",
            vec!["export default function Page() {}".to_string()],
        ));

        assert_eq!(rendered.lines.len(), 3);
        assert!(rendered.lines[0].starts_with(" ╭─ code (tsx) · app/page.tsx · 1 line "));
        assert!(rendered.lines[0].ends_with('╮'));
        assert!(rendered.lines[1].starts_with(" │ export default function Page() {}"));
        assert!(rendered.lines[1].ends_with(" │"));
        assert!(rendered.lines[2].starts_with(" ╰"));
        assert!(rendered.lines[2].ends_with('╯'));
        assert!(
            rendered.lines[0].chars().count() >= 68,
            "code block should read as a full-width terminal panel, not a tiny widget: {:?}",
            rendered.lines
        );
        assert!(!rendered.collapsed);
    }

    #[test]
    fn infers_language_from_path_label() {
        let rendered = render_code_block(CodeBlockInput::new(
            "app/page.tsx",
            vec!["export default function Page() {}".to_string()],
        ));

        assert!(rendered.lines[0].starts_with(" ╭─ code (tsx) · app/page.tsx · 1 line "));
    }

    #[test]
    fn wrapped_code_lines_show_continuation_marker() {
        let rendered = render_code_block(CodeBlockInput::new(
            "tsx app/page.tsx",
            vec![
                r#"<main className="flex min-h-screen flex-col items-center justify-center p-24">"#
                    .to_string(),
            ],
        ));

        assert!(rendered
            .lines
            .iter()
            .any(|line| line.contains("justify-center")));
        assert!(rendered.lines.iter().any(|line| line.contains("↳ p-24")));
        assert!(!rendered
            .lines
            .iter()
            .any(|line| line.trim_end().ends_with("justify-cen │")));
    }
}
