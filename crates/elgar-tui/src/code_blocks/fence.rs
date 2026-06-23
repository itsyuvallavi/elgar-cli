//! Code fence metadata parsing.
//!
//! Fence info becomes the terminal box header: language, optional path/label,
//! and visible line counts.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CodeFenceInfo {
    pub(super) language: Option<String>,
    pub(super) label: Option<String>,
}

impl CodeFenceInfo {
    pub(super) fn parse(info: &str) -> Self {
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

pub(super) fn render_code_header(
    info: &CodeFenceInfo,
    line_count: usize,
    collapsed_shown_lines: Option<usize>,
) -> String {
    let mut parts = vec![primary_header_label(info)];
    if let Some(shown_lines) = collapsed_shown_lines {
        parts.push(line_count_label(line_count));
        parts.push(format!("collapsed, showing {shown_lines}"));
    }
    parts.join(" · ")
}

fn primary_header_label(info: &CodeFenceInfo) -> String {
    info.label
        .as_deref()
        .or(info.language.as_deref())
        .unwrap_or("code")
        .to_string()
}

fn line_count_label(line_count: usize) -> String {
    if line_count == 1 {
        "1 line".to_string()
    } else {
        format!("{line_count} lines")
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
