//! Markdown artifact normalization before line rendering.
//!
//! These helpers clean common provider formatting artifacts while avoiding a
//! full markdown parse.

pub(super) fn normalize_markdown_artifacts(markdown: &str) -> String {
    let normalized = markdown.replace("\r\n", "\n").replace("<br>", "\n");
    let normalized = expand_inline_fenced_code_blocks(&normalized);
    split_inline_bullet_markers(&normalized)
}

fn expand_inline_fenced_code_blocks(text: &str) -> String {
    text.lines()
        .map(expand_inline_fenced_code_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn expand_inline_fenced_code_line(line: &str) -> String {
    let mut rendered = Vec::new();
    let mut rest = line;

    while let Some(start) = rest.find("```") {
        let prefix = rest[..start].trim_end();
        if !prefix.is_empty() {
            rendered.push(prefix.to_string());
        }

        let after_open = &rest[start + 3..];
        let Some(end) = after_open.find("```") else {
            rendered.push(rest.to_string());
            return rendered.join("\n");
        };

        let fenced = after_open[..end].trim();
        let (language, code) = split_inline_fence_content(fenced);
        if language.is_empty() {
            rendered.push("```".to_string());
        } else {
            rendered.push(format!("```{language}"));
        }
        rendered.extend(normalize_inline_code_content(code));
        rendered.push("```".to_string());

        rest = after_open[end + 3..].trim_start();
    }

    if !rest.trim().is_empty() {
        rendered.push(rest.to_string());
    }

    if rendered.is_empty() {
        line.to_string()
    } else {
        rendered.join("\n")
    }
}

fn split_inline_fence_content(fenced: &str) -> (&str, &str) {
    let fenced = fenced.trim();
    let Some((first, rest)) = fenced.split_once(char::is_whitespace) else {
        return (fenced, "");
    };

    if first
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        (first, rest.trim())
    } else {
        ("", fenced)
    }
}

fn normalize_inline_code_content(code: &str) -> Vec<String> {
    let mut normalized = code.trim().to_string();
    for marker in [
        "# 1.",
        "# 2.",
        "# 3.",
        "# 4.",
        "# 5.",
        "# Expected",
        "# Output",
        "# Result",
    ] {
        normalized = normalized.replace(&format!(" {marker}"), &format!("\n{marker}"));
    }

    normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn split_inline_bullet_markers(text: &str) -> String {
    let mut in_code_block = false;
    text.lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_code_block = !in_code_block;
                return line.to_string();
            }
            if in_code_block {
                return line.to_string();
            }
            split_inline_bullet_line(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn split_inline_bullet_line(line: &str) -> String {
    let line = line.replace(": - ", ":\n- ");

    if (line.contains("\n- ") && line.contains(" - ")) || line.matches(" - ").count() >= 2 {
        line.replace(" - ", "\n- ")
    } else {
        line
    }
}
