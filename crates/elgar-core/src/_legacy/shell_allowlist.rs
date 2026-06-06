use crate::action::ShellCommandAction;

pub(crate) fn is_read_only_shell_command(action: &ShellCommandAction) -> bool {
    if read_only_command_with_error_fallback(&action.command) {
        return true;
    }

    if let Some(parts) = read_only_head_pipeline_parts(&action.command) {
        return parts
            .iter()
            .all(|part| read_only_command_with_optional_stderr_redirect(part));
    }

    read_only_command_with_optional_stderr_redirect(&action.command)
}

fn read_only_command_with_error_fallback(command: &str) -> bool {
    let Some(parts) = split_unquoted_or(command) else {
        return false;
    };
    let [left, right] = parts.as_slice() else {
        return false;
    };
    read_only_command_with_optional_stderr_redirect(left) && literal_echo_command(right)
}

fn read_only_command_with_optional_stderr_redirect(command: &str) -> bool {
    let command = strip_trailing_stderr_redirect(command.trim());
    shell_words(command).is_some_and(|words| !words.is_empty() && read_only_words(&words))
}

fn strip_trailing_stderr_redirect(command: &str) -> &str {
    for suffix in ["2>/dev/null", "2> /dev/null", "2>&1", "2> &1"] {
        if let Some(stripped) = command.strip_suffix(suffix) {
            return stripped.trim_end();
        }
    }
    command
}

fn literal_echo_command(command: &str) -> bool {
    shell_words(command).is_some_and(|words| {
        matches!(words.first().map(String::as_str), Some("echo")) && words.len() >= 2
    })
}

fn read_only_head_pipeline_parts(command: &str) -> Option<[String; 2]> {
    let parts = split_unquoted_pipe(command)?;
    let [left, right] = parts.as_slice() else {
        return None;
    };
    let right_words = shell_words(right)?;
    if !is_limited_head_command(&right_words) {
        return None;
    }
    Some([left.to_string(), right.to_string()])
}

fn split_unquoted_or(command: &str) -> Option<Vec<String>> {
    split_unquoted_operator(command, "||")
}

fn split_unquoted_pipe(command: &str) -> Option<Vec<String>> {
    split_unquoted_operator(command, "|")
}

fn split_unquoted_operator(command: &str, operator: &str) -> Option<Vec<String>> {
    if command.trim().is_empty() || command.chars().any(|ch| matches!(ch, '\n' | '\r')) {
        return None;
    }

    let mut parts = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else if matches!(ch, '$' | '`' | '\\') {
                return None;
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            ch if operator == "|" && ch == '|' => {
                let part = current.trim();
                if part.is_empty() {
                    return None;
                }
                parts.push(part.to_string());
                current.clear();
            }
            ch if operator == "||" && ch == '|' && chars.peek() == Some(&'|') => {
                chars.next();
                let part = current.trim();
                if part.is_empty() {
                    return None;
                }
                parts.push(part.to_string());
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return None;
    }
    let part = current.trim();
    if part.is_empty() {
        return None;
    }
    parts.push(part.to_string());
    Some(parts)
}

fn is_limited_head_command(words: &[String]) -> bool {
    match words {
        [program, flag] if program == "head" => flag
            .strip_prefix('-')
            .is_some_and(|limit| !limit.is_empty() && limit.chars().all(|ch| ch.is_ascii_digit())),
        [program, flag, value] if program == "head" && flag == "-n" => {
            !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
        }
        _ => false,
    }
}

fn shell_words(command: &str) -> Option<Vec<String>> {
    if command.trim().is_empty() || command.chars().any(|ch| matches!(ch, '\n' | '\r')) {
        return None;
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else if matches!(ch, '$' | '`' | '\\') {
                return None;
            } else {
                current.push(ch);
            }
            continue;
        }

        match ch {
            '\'' | '"' => quote = Some(ch),
            ' ' | '\t' => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            ';' | '|' | '&' | '>' | '<' | '(' | ')' | '{' | '}' | '$' | '`' | '\\' => {
                return None;
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

fn read_only_words(words: &[String]) -> bool {
    let command = words[0].as_str();
    let args = &words[1..];
    match command {
        "pwd" => args.is_empty(),
        "ls" | "cat" | "head" | "tail" | "wc" | "rg" | "grep" | "du" | "tree" => {
            no_known_write_flags(args)
        }
        "sed" => no_known_write_flags(args) && !has_any_arg(args, &["-i", "--in-place"]),
        "find" => {
            no_known_write_flags(args)
                && !has_any_arg(
                    args,
                    &[
                        "-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprintf",
                    ],
                )
        }
        "git" => git_read_only(args),
        _ => false,
    }
}

fn git_read_only(args: &[String]) -> bool {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return false;
    };
    if !no_known_write_flags(args) {
        return false;
    }
    matches!(
        subcommand,
        "branch" | "diff" | "grep" | "log" | "ls-files" | "rev-parse" | "show" | "status" | "tag"
    )
}

fn no_known_write_flags(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-o" | "--output" | "--output-document" | "--write" | "--delete"
        ) || arg.starts_with("--output=")
            || arg.starts_with("-o")
    })
}

fn has_any_arg(args: &[String], blocked: &[&str]) -> bool {
    args.iter().any(|arg| blocked.contains(&arg.as_str()))
}

#[cfg(test)]
mod tests {
    use crate::{action::ShellCommandAction, shell_allowlist::is_read_only_shell_command};

    fn safe(command: &str) -> bool {
        is_read_only_shell_command(&ShellCommandAction::new(command, "."))
    }

    #[test]
    fn allows_simple_read_only_commands() {
        assert!(safe("pwd"));
        assert!(safe("ls -la src"));
        assert!(safe("cat README.md"));
        assert!(safe("cat package.json 2>/dev/null"));
        assert!(safe(
            "cat package.json 2>/dev/null || echo \"no package.json\""
        ));
        assert!(safe("cat app/page.tsx 2>&1 || echo \"FILE_NOT_FOUND\""));
        assert!(safe("rg \"ShellCommand\" crates/elgar-core/src"));
        assert!(safe("git status --short"));
        assert!(safe("git diff -- crates/elgar-core/src/action.rs"));
        assert!(safe(
            "find . -maxdepth 3 -not -path '*/node_modules/*' -not -path '*/.git/*' | head -80"
        ));
        assert!(safe("rg --files | head -n 80"));
    }

    #[test]
    fn rejects_mutating_and_complex_shell_commands() {
        assert!(!safe("mkdir demo"));
        assert!(!safe("rm -rf demo"));
        assert!(!safe("cat README.md > copy.md"));
        assert!(!safe("cat README.md 2>/dev/null || rm -rf demo"));
        assert!(!safe("cat README.md 2>/dev/null || echo $(pwd)"));
        assert!(!safe("ls | cat"));
        assert!(!safe("find . -delete | head -80"));
        assert!(!safe("find . -maxdepth 3 | head foo"));
        assert!(!safe("find . -maxdepth 3 | head -80 | cat"));
        assert!(!safe("echo $(pwd)"));
        assert!(!safe("sed -i '' 's/a/b/' file.txt"));
        assert!(!safe("find . -delete"));
        assert!(!safe("git checkout main"));
    }
}
