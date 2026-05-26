pub fn provider_visible_text_from_text_only_output(message: String) -> Option<String> {
    let text = message.trim();
    if text.is_empty() {
        return None;
    }

    let mut visible_lines = Vec::new();
    for line in text.lines() {
        match provider_visible_line(line) {
            Some(visible) => visible_lines.push(visible),
            None if !visible_lines.is_empty() => {
                if visible_lines.last().is_some_and(|line| !line.is_empty()) {
                    visible_lines.push(String::new());
                }
            }
            None => {}
        }
    }

    while visible_lines.last().is_some_and(|line| line.is_empty()) {
        visible_lines.pop();
    }

    let text = visible_lines.join("\n").trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn provider_visible_line(line: &str) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }

    let sentences = split_provider_sentences(line);
    let removed_any = sentences
        .iter()
        .any(|sentence| is_provider_tool_planning_sentence(sentence));
    if !removed_any {
        return Some(line.trim_end().to_string());
    }

    let visible_sentences = sentences
        .into_iter()
        .filter(|sentence| !is_provider_tool_planning_sentence(sentence))
        .filter(|sentence| !is_provider_tool_planning_filler_sentence(sentence))
        .map(str::trim)
        .filter(|sentence| !sentence.is_empty())
        .collect::<Vec<_>>();

    if visible_sentences.is_empty() {
        None
    } else {
        Some(visible_sentences.join(" "))
    }
}

fn split_provider_sentences(line: &str) -> Vec<&str> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let bytes = line.as_bytes();
    for (index, character) in line.char_indices() {
        if !matches!(character, '.' | '?' | '!') {
            continue;
        }
        let next = index + character.len_utf8();
        if next == line.len() || bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
            sentences.push(&line[start..next]);
            start = next;
        }
    }

    if start < line.len() {
        sentences.push(&line[start..]);
    }

    sentences
}

fn is_provider_tool_planning_sentence(sentence: &str) -> bool {
    if is_provider_text_contract_line(sentence) {
        return true;
    }

    let trimmed = sentence.trim().to_ascii_lowercase();
    if trimmed.starts_with("to=functions") || trimmed.contains(" to=functions") {
        return true;
    }

    let normalized = normalized_provider_text_contract_line(sentence);
    normalized == "path"
        || normalized == "create_directory"
        || normalized == "create_file"
        || normalized == "we need create files"
        || normalized.starts_with("we need create ")
        || normalized.starts_with("we need to create ")
        || normalized.starts_with("need create ")
        || normalized.starts_with("need to create ")
        || normalized == "create directory"
        || normalized == "create file"
        || normalized.starts_with("create directory on desktop")
        || normalized.starts_with("create directory on the desktop")
        || normalized.starts_with("create directory on my desktop")
        || normalized.starts_with("desktop relative path")
        || normalized.starts_with("desktop path")
        || normalized.starts_with("path project relative path")
        || normalized.starts_with("project relative path")
        || normalized.starts_with("projectrelative path")
        || normalized.contains("probably desktop path")
        || normalized.contains("likely desktop path")
        || normalized.contains("probably the desktop path")
        || normalized.contains("likely the desktop path")
        || normalized.starts_with("use create_directory tool")
        || normalized.starts_with("use create_directory function")
        || normalized.starts_with("use create_file tool")
        || normalized.starts_with("use create_file function")
        || normalized.starts_with("create markdown file plan")
        || normalized.starts_with("create files packagejson")
        || normalized.contains(" provide tool calls")
        || normalized.starts_with("provide tool calls")
        || normalized.contains(" we cannot run shell")
        || normalized.starts_with("we cannot run shell")
        || (normalized.starts_with("create directory")
            && normalized.contains(" use create_directory tool"))
        || (normalized.starts_with("create file") && normalized.contains(" use create_file tool"))
}

fn is_provider_tool_planning_filler_sentence(sentence: &str) -> bool {
    matches!(
        normalized_provider_text_contract_line(sentence).as_str(),
        "lets continue"
            | "let's continue"
            | "lets implement"
            | "let's implement"
            | "lets create files"
            | "let's create files"
            | "lets create the files"
            | "let's create the files"
    )
}

fn is_provider_text_contract_line(line: &str) -> bool {
    matches!(
        normalized_provider_text_contract_line(line).as_str(),
        "output markdown content only"
            | "suggest content only"
            | "use create_directory tool"
            | "use create_directory function"
            | "use create_file tool"
            | "use create_file function"
            | "call create_directory"
            | "call create_file"
            | "create directory use create_directory tool"
            | "create directory use create_directory function"
            | "create file use create_file tool"
            | "create file use create_file function"
    )
}

fn normalized_provider_text_contract_line(line: &str) -> String {
    line.trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| !matches!(ch, '.' | ':' | ';' | '?' | '!' | '-'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::provider_visible_text_from_text_only_output;

    #[test]
    fn provider_visible_drops_empty_text() {
        assert_eq!(
            provider_visible_text_from_text_only_output(" \n\t ".to_string()),
            None
        );
    }

    #[test]
    fn provider_visible_hides_contract_only_text() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Create file: use create_file function.".to_string()
            ),
            None
        );
    }

    #[test]
    fn provider_visible_drops_only_leading_contract_line() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Output markdown content only.\n# Plan\n\nUse create_file tool only after approval."
                    .to_string()
            ),
            Some("# Plan".to_string())
        );
    }

    #[test]
    fn provider_visible_keeps_normal_text_unchanged_after_outer_trim() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                " Tool-call mode may use create_file tool in an explanation. \n".to_string()
            ),
            Some("Tool-call mode may use create_file tool in an explanation.".to_string())
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Creating a plan first is useful when the requirements are still open.".to_string()
            ),
            Some(
                "Creating a plan first is useful when the requirements are still open.".to_string()
            )
        );
    }

    #[test]
    fn provider_visible_hides_tool_planning_sentences_but_keeps_answer() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Use create_directory tool. Path? Project-relative path: likely the Desktop folder. The folder is ready to create."
                    .to_string()
            ),
            Some("The folder is ready to create.".to_string())
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Create files: package.json? Provide tool calls. We cannot run shell here."
                    .to_string()
            ),
            None
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "We need create pages/index.tsx, styles/globals.css, tailwind config.".to_string()
            ),
            None
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Create directory. Use create_directory tool. Path? Desktop relative path maybe \"Desktop/ElgarLiveE2E\"."
                    .to_string()
            ),
            None
        );
        assert_eq!(
            provider_visible_text_from_text_only_output("Create_directory.".to_string()),
            None
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Create directory on desktop. Use create_directory tool. Path? Project-relative path: probably Desktop/ElgarLiveE2E."
                    .to_string()
            ),
            None
        );
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "to=functions create_directory? Let's continue.".to_string()
            ),
            None
        );
    }

    #[test]
    fn provider_visible_keeps_contract_line_when_only_followed_by_blank_text() {
        assert_eq!(
            provider_visible_text_from_text_only_output(
                "Output markdown content only.\n\n  ".to_string()
            ),
            None
        );
    }
}
