//! Language normalization for code token styling.

pub(super) fn normalized_code_language(language: Option<&str>) -> Option<String> {
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

pub(super) fn comment_marker(language: &str) -> Option<&'static str> {
    match language {
        "bash" | "python" | "toml" | "yaml" => Some("#"),
        "javascript" | "rust" | "typescript" => Some("//"),
        _ => None,
    }
}
