pub(crate) fn render_assistant_markdown(markdown: &str) -> String {
    let normalized = normalize_markdown_artifacts(markdown);
    let lines: Vec<&str> = normalized.lines().collect();
    let mut rendered = Vec::new();
    let mut index = 0;
    let mut code_block: Option<CodeBlock> = None;
    let mut skip_blank_after_code_block = false;

    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if let Some(block) = code_block.as_mut() {
            if trimmed.starts_with("```") {
                let block = code_block
                    .take()
                    .expect("code block exists while rendering code line");
                trim_trailing_blank_lines(&mut rendered);
                render_code_block(&mut rendered, block);
                skip_blank_after_code_block = true;
            } else {
                block.lines.push(line.to_string());
            }
            index += 1;
            continue;
        }

        if let Some(language) = trimmed.strip_prefix("```") {
            code_block = Some(CodeBlock::new(language.trim()));
            index += 1;
            continue;
        }

        if skip_blank_after_code_block && trimmed.is_empty() {
            index += 1;
            continue;
        }
        skip_blank_after_code_block = false;

        if trimmed.is_empty() {
            index += 1;
            continue;
        }

        if is_table_start(&lines, index) {
            let (table, next_index) = render_table(&lines, index);
            rendered.extend(table);
            index = next_index;
            continue;
        }

        if let Some(list_line) = render_list_line(line) {
            rendered.push(list_line);
        } else if is_preformatted_line(line) {
            rendered.push(render_preformatted_line(line));
        } else {
            rendered.push(render_plain_line(line));
        }

        index += 1;
    }

    if let Some(block) = code_block {
        trim_trailing_blank_lines(&mut rendered);
        render_code_block(&mut rendered, block);
    }

    rendered.join("\n")
}

fn normalize_markdown_artifacts(markdown: &str) -> String {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodeBlock {
    language: String,
    lines: Vec<String>,
}

impl CodeBlock {
    fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
            lines: Vec::new(),
        }
    }
}

fn render_code_block(rendered: &mut Vec<String>, block: CodeBlock) {
    if block.language.is_empty() {
        rendered.push("code:".to_string());
    } else {
        rendered.push(format!("code ({}):", block.language));
    }

    let Some(start) = block.lines.iter().position(|line| !line.trim().is_empty()) else {
        return;
    };
    let end = block
        .lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(start);

    for line in &block.lines[start..end] {
        if line.trim().is_empty() {
            continue;
        }
        rendered.push(format!("    {line}"));
    }
}

fn trim_trailing_blank_lines(lines: &mut Vec<String>) {
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
}

fn render_list_line(line: &str) -> Option<String> {
    let trimmed_start = line.trim_start();
    let indent = line.len().saturating_sub(trimmed_start.len());
    let rendered_indent = " ".repeat(indent.min(6));

    for marker in ["- ", "* ", "+ "] {
        if let Some(item) = trimmed_start.strip_prefix(marker) {
            return Some(format!("{rendered_indent}- {}", render_inline(item.trim())));
        }
    }

    let (number, item) = trimmed_start.split_once(". ")?;
    if number.chars().all(|character| character.is_ascii_digit()) && !number.is_empty() {
        Some(format!(
            "{rendered_indent}{number}. {}",
            render_inline(item.trim())
        ))
    } else {
        None
    }
}

fn is_preformatted_line(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

fn render_preformatted_line(line: &str) -> String {
    if let Some(rest) = line.strip_prefix('\t') {
        format!("    {rest}")
    } else {
        line.to_string()
    }
}

fn is_table_start(lines: &[&str], index: usize) -> bool {
    index + 1 < lines.len() && is_table_row(lines[index]) && is_table_separator(lines[index + 1])
}

fn render_table(lines: &[&str], start: usize) -> (Vec<String>, usize) {
    let mut rows = Vec::new();
    let mut index = start;

    while index < lines.len() && is_table_row(lines[index]) {
        if !is_table_separator(lines[index]) {
            rows.push(table_cells(lines[index]));
        }
        index += 1;
    }

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; column_count];
    for row in &rows {
        for (column, cell) in row.iter().enumerate() {
            widths[column] = widths[column].max(cell.len());
        }
    }

    let mut rendered = Vec::new();
    for (row_index, row) in rows.iter().enumerate() {
        rendered.push(render_table_row(row, &widths));
        if row_index == 0 && rows.len() > 1 {
            rendered.push(render_table_rule(&widths));
        }
    }

    (rendered, index)
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains('|') && !trimmed.is_empty()
}

fn is_table_separator(line: &str) -> bool {
    let cells = table_cells(line);
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let trimmed = cell.trim_matches(':');
            trimmed.len() >= 3 && trimmed.chars().all(|character| character == '-')
        })
}

fn table_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|cell| render_inline(cell.trim()))
        .collect()
}

fn render_table_row(row: &[String], widths: &[usize]) -> String {
    let cells = widths
        .iter()
        .enumerate()
        .map(|(column, width)| {
            let cell = row.get(column).map(String::as_str).unwrap_or("");
            format!("{cell:<width$}")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    format!("  {cells}")
}

fn render_table_rule(widths: &[usize]) -> String {
    let cells = widths
        .iter()
        .map(|width| "-".repeat((*width).max(3)))
        .collect::<Vec<_>>()
        .join("-+-");
    format!("  {cells}")
}

fn render_plain_line(line: &str) -> String {
    render_inline(line.trim_end())
}

fn render_inline(line: &str) -> String {
    line.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .replace("\\_", "_")
}

#[cfg(test)]
mod tests {
    use super::render_assistant_markdown;

    #[test]
    fn renders_plain_text_without_changing_content() {
        assert_eq!(
            render_assistant_markdown("Plain assistant text.\nSecond line."),
            "Plain assistant text.\nSecond line."
        );
    }

    #[test]
    fn renders_code_blocks_without_fences() {
        let rendered = render_assistant_markdown("Use this:\n```rust\nfn main() {}\n```");

        assert_eq!(rendered, "Use this:\ncode (rust):\n    fn main() {}");
        assert!(!rendered.contains("```"));
    }

    #[test]
    fn renders_code_blocks_with_compact_fence_spacing() {
        let rendered =
            render_assistant_markdown("Use this:\n\n```rust\n\nfn main() {}\n\n```\n\nDone.");

        assert_eq!(rendered, "Use this:\ncode (rust):\n    fn main() {}\nDone.");
    }

    #[test]
    fn compacts_blank_lines_inside_fenced_code_blocks() {
        let rendered = render_assistant_markdown(
            "code:\n```python\nimport json\n\n\ndef main():\n\n    print(\"ok\")\n\n\nmain()\n```",
        );

        assert_eq!(
            rendered,
            "code:\ncode (python):\n    import json\n    def main():\n        print(\"ok\")\n    main()"
        );
    }

    #[test]
    fn compacts_blank_lines_between_plain_blocks_and_lists() {
        let rendered = render_assistant_markdown(
            "Sure! Let me suggest a small folder structure.\n\ncode:\n\n    project/\n\n    src/\n\nWhat to do:\n\n1. Create directories.\n\n2. Move files.\n\nOnce you approve, I can generate commands.",
        );

        assert_eq!(
            rendered,
            "Sure! Let me suggest a small folder structure.\ncode:\n    project/\n    src/\nWhat to do:\n1. Create directories.\n2. Move files.\nOnce you approve, I can generate commands."
        );
    }

    #[test]
    fn expands_inline_fenced_code_blocks_into_readable_blocks() {
        let rendered = render_assistant_markdown(
            "Use this: ```bash # 1. Start lm-studio --port 1234 # 2. Check curl http://127.0.0.1:1234/v1/health ``` Done.",
        );

        assert_eq!(
            rendered,
            "Use this:\ncode (bash):\n    # 1. Start lm-studio --port 1234\n    # 2. Check curl http://127.0.0.1:1234/v1/health\nDone."
        );
    }

    #[test]
    fn renders_lists_with_clean_markers() {
        let rendered = render_assistant_markdown("- **one**\n  * two\n1. `three`");

        assert_eq!(rendered, "- one\n  - two\n1. three");
    }

    #[test]
    fn expands_inline_bullet_markers_into_list_lines() {
        let rendered = render_assistant_markdown(
            "I can: - Answer questions. - Summarise documents. - Generate config files.",
        );

        assert_eq!(
            rendered,
            "I can:\n- Answer questions.\n- Summarise documents.\n- Generate config files."
        );
    }

    #[test]
    fn renders_tables_without_markdown_separator_rows() {
        let rendered = render_assistant_markdown("| File | State |\n| --- | --- |\n| a.rs | ok |");

        assert_eq!(rendered, "  File | State\n  -----+------\n  a.rs | ok   ");
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn leaves_table_like_text_without_separator_as_plain_text() {
        let rendered = render_assistant_markdown("| File | State |\n| a.rs | ok |");

        assert_eq!(rendered, "| File | State |\n| a.rs | ok |");
    }

    #[test]
    fn renders_preformatted_blocks_as_indented_text() {
        let rendered = render_assistant_markdown("tree:\n    src/\n      lib.rs");

        assert_eq!(rendered, "tree:\n    src/\n      lib.rs");
    }
}
