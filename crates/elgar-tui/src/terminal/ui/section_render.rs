//! Quiet section container rendering for assistant responses.
//!
//! Rendering is display-only: it keeps the assistant text selectable and does
//! not decide tools, approvals, or runtime behavior.

use super::sections::ResponseSection;

const MIN_CONTENT_WIDTH: usize = 36;
const MAX_CONTENT_WIDTH: usize = 84;

/// Render assistant sections inside one compact response container.
pub(crate) fn render_response_sections(sections: &[ResponseSection]) -> String {
    let content_width = content_width(sections);
    let mut lines = vec![top_line(content_width)];

    for (index, section) in sections.iter().enumerate() {
        if index > 0 {
            lines.push(body_line("", content_width));
        }
        lines.push(body_line(&section.title, content_width));
        for line in section.lines.iter().filter(|line| !line.trim().is_empty()) {
            for segment in split_to_width(
                &indent_content(line),
                content_width,
                continuation_prefix(line),
            ) {
                lines.push(body_line(&segment, content_width));
            }
        }
    }

    lines.push(bottom_line(content_width));
    lines.join("\n")
}

fn content_width(sections: &[ResponseSection]) -> usize {
    let natural = sections
        .iter()
        .flat_map(|section| {
            std::iter::once(section.title.as_str()).chain(section.lines.iter().map(String::as_str))
        })
        .map(|line| indent_content(line).chars().count())
        .max()
        .unwrap_or(MIN_CONTENT_WIDTH);

    natural.clamp(MIN_CONTENT_WIDTH, MAX_CONTENT_WIDTH)
}

fn indent_content(line: &str) -> String {
    if line.trim().is_empty() {
        String::new()
    } else {
        format!("  {}", line.trim_start())
    }
}

fn continuation_prefix(line: &str) -> String {
    let indented = indent_content(line);
    let marker_prefix = ["  - ", "  * ", "  + "]
        .iter()
        .find(|prefix| indented.starts_with(**prefix))
        .copied();

    if let Some(prefix) = marker_prefix {
        " ".repeat(prefix.chars().count())
    } else {
        let leading = indented.chars().take_while(|ch| ch.is_whitespace()).count();
        " ".repeat(leading)
    }
}

fn top_line(content_width: usize) -> String {
    let title = " response ";
    let fill = "─".repeat(content_width.saturating_sub(title.chars().count()) + 3);
    format!(" ╭─{title}{fill}╮")
}

fn body_line(line: &str, content_width: usize) -> String {
    let visible = line.chars().count();
    let padding = content_width.saturating_sub(visible);
    format!(" │ {line}{} │", " ".repeat(padding))
}

fn bottom_line(content_width: usize) -> String {
    format!(" ╰{}╯", "─".repeat(content_width + 3))
}

fn split_to_width(line: &str, width: usize, continuation_prefix: String) -> Vec<String> {
    if line.chars().count() <= width {
        return vec![line.to_string()];
    }

    let leading_prefix = line
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    let mut segments = Vec::new();
    let mut current = leading_prefix;
    for word in line.trim_start().split_whitespace() {
        let candidate = if current.trim().is_empty() {
            format!("{current}{word}")
        } else {
            format!("{current} {word}")
        };

        if candidate.chars().count() > width && !current.trim().is_empty() {
            push_wrapped_segment(&mut segments, current, &continuation_prefix);
            current = word.to_string();
        } else if candidate.chars().count() > width {
            segments.extend(hard_split(word, width));
            current.clear();
        } else {
            current = candidate;
        }
    }
    if !current.is_empty() {
        push_wrapped_segment(&mut segments, current, &continuation_prefix);
    }
    segments
}

fn push_wrapped_segment(segments: &mut Vec<String>, segment: String, continuation_prefix: &str) {
    if segments.is_empty() {
        segments.push(segment);
    } else {
        segments.push(format!("{continuation_prefix}{segment}"));
    }
}

fn hard_split(word: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in word.chars() {
        current.push(character);
        if current.chars().count() >= width {
            chunks.push(current);
            current = String::new();
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::{render_response_sections, ResponseSection};

    #[test]
    fn renders_sections_in_one_container() {
        let rendered = render_response_sections(&[
            ResponseSection {
                title: "Summary".to_string(),
                lines: vec!["Todo app created.".to_string()],
            },
            ResponseSection {
                title: "Files".to_string(),
                lines: vec!["- `app/page.tsx`".to_string()],
            },
        ]);

        assert!(rendered.starts_with(" ╭─ response "));
        assert!(rendered.contains("│ Summary"));
        assert!(rendered.contains("│   Todo app created."));
        assert!(rendered.contains("│ Files"));
        assert!(rendered.contains("`app/page.tsx`"));
        assert!(rendered.ends_with('╯'));
    }

    #[test]
    fn aligns_wrapped_bullet_continuations() {
        let rendered = render_response_sections(&[ResponseSection {
            title: "Verification".to_string(),
            lines: vec![
                "- Run `npm start` and manually test adding completing and deleting a todo item in the browser."
                    .to_string(),
            ],
        }]);

        let wrapped = rendered
            .lines()
            .filter(|line| line.contains("Run `npm start`") || line.contains("the browser"))
            .collect::<Vec<_>>();

        assert_eq!(wrapped.len(), 2, "{wrapped:#?}");
        assert!(wrapped[0].contains("│   - Run `npm start`"), "{wrapped:#?}");
        assert!(wrapped[1].contains("│     the browser."), "{wrapped:#?}");
    }
}
