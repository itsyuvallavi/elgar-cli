use crate::{
    action::{ActionRequest, ShellCommandAction},
    agent_tool_output::ResolvedAgentToolOutput,
    session::Session,
};

pub(crate) fn guard_shell_inspection_tool_outputs(
    session: &mut Session,
    outputs: Vec<ResolvedAgentToolOutput>,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action) => {
                let ActionRequest::ShellCommand(shell) = action.request else {
                    return ResolvedAgentToolOutput::Action(action);
                };
                let (shell, rewrite_reason) = rewrite_heavy_shell_inspection(shell);
                if let Some(reason) = rewrite_reason {
                    session.push_reasoning_runtime_check(reason);
                }
                action.request = ActionRequest::ShellCommand(shell);
                action.target_label = action.request.approval_target();
                ResolvedAgentToolOutput::Action(action)
            }
            other => other,
        })
        .collect()
}

fn rewrite_heavy_shell_inspection(
    mut shell: ShellCommandAction,
) -> (ShellCommandAction, Option<String>) {
    if let Some(command) = rewrite_missing_file_cat_fallback(&shell.command) {
        let original = shell.command.clone();
        shell.command = command;
        return (
            shell,
            Some(format!(
                "rewrote read-only shell fallback `{}` to direct file read",
                original.trim()
            )),
        );
    }

    if !shell_command_is_heavy_project_listing(&shell.command) {
        return (shell, None);
    }

    let original = shell.command.clone();
    shell.command = safe_project_listing_command().to_string();
    shell.output_caps.stdout_bytes = shell.output_caps.stdout_bytes.min(8 * 1024);
    (
        shell,
        Some(format!(
            "rewrote heavy project listing shell command `{}` to bounded project inspection",
            original.trim()
        )),
    )
}

fn rewrite_missing_file_cat_fallback(command: &str) -> Option<String> {
    let trimmed = command.trim();
    for marker in [
        " 2>/dev/null || echo \"MISSING\"",
        " 2>/dev/null || echo 'MISSING'",
        " 2>/dev/null || echo \"NOT FOUND\"",
        " 2>/dev/null || echo 'NOT FOUND'",
    ] {
        let Some(prefix) = trimmed.strip_suffix(marker) else {
            continue;
        };
        let words = simple_shell_words(prefix)?;
        if matches!(words.as_slice(), [program, _path] if program == "cat") {
            return Some(prefix.to_string());
        }
    }
    None
}

fn shell_command_is_heavy_project_listing(command: &str) -> bool {
    let command = strip_project_listing_pipe(command).unwrap_or(command);
    let Some(words) = simple_shell_words(command) else {
        return false;
    };
    let Some(program) = words.first().map(String::as_str) else {
        return false;
    };

    match program {
        "ls" => words[1..]
            .iter()
            .any(|word| word.starts_with('-') && word.chars().skip(1).any(|ch| ch == 'R')),
        "find" => words
            .get(1)
            .is_some_and(|target| matches!(target.as_str(), "." | "./")),
        _ => false,
    }
}

pub(crate) fn shell_command_is_project_listing(command: &str) -> bool {
    if command.trim() == safe_project_listing_command() {
        return true;
    }
    let command = strip_project_listing_pipe(command).unwrap_or(command);
    let Some(words) = simple_shell_words(command) else {
        return false;
    };
    matches!(
        words.first().map(String::as_str),
        Some("find" | "ls" | "tree")
    )
}

pub(crate) fn shell_command_is_direct_file_read(command: &str) -> bool {
    let command = strip_shell_fallback(command);
    let Some(mut words) = simple_shell_words(command) else {
        return false;
    };
    strip_trailing_shell_redirect_words(&mut words);
    matches!(
        words.as_slice(),
        [program, _path] if matches!(program.as_str(), "cat" | "bat")
    ) || matches!(
        words.as_slice(),
        [program, _range, _path] if program == "sed"
    ) || matches!(
        words.as_slice(),
        [program, flag, _range, _path] if program == "sed" && flag == "-n"
    )
}

fn strip_shell_fallback(command: &str) -> &str {
    command
        .split_once("||")
        .map(|(left, _)| left.trim())
        .unwrap_or(command.trim())
}

fn strip_trailing_shell_redirect_words(words: &mut Vec<String>) {
    while words.last().is_some_and(|word| {
        word == "2>/dev/null"
            || word == "2>&1"
            || word == "1>/dev/null"
            || word == ">/dev/null"
            || word.starts_with("2>")
            || word.starts_with("1>")
    }) {
        words.pop();
    }
}

fn strip_project_listing_pipe(command: &str) -> Option<&str> {
    let (left, right) = command.split_once('|')?;
    if right.contains('|') {
        return None;
    }
    let right_words = simple_shell_words(right.trim())?;
    if !matches_limited_head_words(&right_words) && !matches_exact_sort_words(&right_words) {
        return None;
    }
    Some(left.trim())
}

fn matches_exact_sort_words(words: &[String]) -> bool {
    matches!(words, [program] if program == "sort")
}

fn matches_limited_head_words(words: &[String]) -> bool {
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

fn safe_project_listing_command() -> &'static str {
    "find . -maxdepth 3 -not -path './.git' -not -path './.git/*' -not -path './node_modules' -not -path './node_modules/*' -not -path './.next' -not -path './.next/*' -not -path './target' -not -path './target/*' -not -path './dist' -not -path './dist/*' -not -path './build' -not -path './build/*' -not -path './coverage' -not -path './coverage/*' -print"
}

fn simple_shell_words(command: &str) -> Option<Vec<String>> {
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
