//! Assistant response section parsing.
//!
//! This module detects lightweight display sections in already-rendered
//! assistant text. It does not infer runtime truth or change model behavior.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResponseSection {
    pub(crate) title: String,
    pub(crate) lines: Vec<String>,
}

/// Parse clear assistant response sections from terminal-rendered text.
pub(crate) fn parse_response_sections(text: &str) -> Option<Vec<ResponseSection>> {
    let mut sections = Vec::new();
    let mut current: Option<ResponseSection> = None;
    let mut prelude = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if is_box_or_code_line(line) {
            push_content_line(&mut current, &mut prelude, line);
            continue;
        }

        if let Some(title) = section_title(line) {
            if let Some(section) = current.take() {
                sections.push(section);
            }
            current = Some(ResponseSection {
                title,
                lines: Vec::new(),
            });
        } else {
            push_content_line(&mut current, &mut prelude, line);
        }
    }

    if let Some(section) = current {
        sections.push(section);
    }

    if sections.len() < 2 {
        return None;
    }

    if !prelude.is_empty() {
        sections.insert(
            0,
            ResponseSection {
                title: "Summary".to_string(),
                lines: prelude,
            },
        );
    }

    Some(sections)
}

fn push_content_line(current: &mut Option<ResponseSection>, prelude: &mut Vec<String>, line: &str) {
    match current.as_mut() {
        Some(section) => section.lines.push(line.to_string()),
        None => prelude.push(line.to_string()),
    }
}

fn section_title(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with(['-', '*', '+']) {
        return None;
    }
    if trimmed.chars().count() > 48 {
        return None;
    }

    let heading = markdown_heading_title(trimmed)
        .or_else(|| colon_heading_title(trimmed))
        .or_else(|| plain_heading_title(trimmed))?;

    if heading.chars().count() > 48 {
        None
    } else {
        Some(heading)
    }
}

fn markdown_heading_title(line: &str) -> Option<String> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=4).contains(&hashes) {
        return None;
    }
    let title = line[hashes..].trim();
    title_from_candidate(title)
}

fn colon_heading_title(line: &str) -> Option<String> {
    let title = line.strip_suffix(':')?.trim();
    title_from_candidate(title)
}

fn plain_heading_title(line: &str) -> Option<String> {
    if line.contains(['.', ',', ';', '`', '/', '\\', '$']) {
        return None;
    }
    if !is_common_section_title(line) {
        return None;
    }
    title_from_candidate(line)
}

fn is_common_section_title(line: &str) -> bool {
    matches!(
        line.to_ascii_lowercase().as_str(),
        "summary"
            | "files"
            | "commands"
            | "verification"
            | "features"
            | "notes"
            | "next steps"
            | "known limitations"
            | "changed files"
            | "tests"
            | "results"
    )
}

fn title_from_candidate(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.contains('|') {
        return None;
    }
    Some(trimmed.to_string())
}

fn is_box_or_code_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(['╭', '│', '╰'])
}

#[cfg(test)]
mod tests {
    use super::parse_response_sections;

    #[test]
    fn leaves_plain_answer_unsectioned() {
        assert!(parse_response_sections("Hello. I can help.").is_none());
    }

    #[test]
    fn parses_markdown_headings() {
        let sections = parse_response_sections("# Summary\nDone\n## Files\n- `app/page.tsx`")
            .expect("sections");

        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].title, "Summary");
        assert_eq!(sections[1].title, "Files");
        assert_eq!(sections[1].lines, vec!["- `app/page.tsx`"]);
    }

    #[test]
    fn parses_plain_section_titles() {
        let sections = parse_response_sections("Summary\nBuilt app\nVerification\nbuild passed")
            .expect("sections");

        assert_eq!(sections[0].title, "Summary");
        assert_eq!(sections[1].title, "Verification");
    }
}
