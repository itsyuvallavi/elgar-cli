use crate::action::ShellCommandAction;

pub(crate) fn is_read_only_shell_command(action: &ShellCommandAction) -> bool {
    let Some(words) = shell_words(&action.command) else {
        return false;
    };
    if words.is_empty() {
        return false;
    }
    read_only_words(&words)
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
        assert!(safe("rg \"ShellCommand\" crates/elgar-core/src"));
        assert!(safe("git status --short"));
        assert!(safe("git diff -- crates/elgar-core/src/action.rs"));
    }

    #[test]
    fn rejects_mutating_and_complex_shell_commands() {
        assert!(!safe("mkdir demo"));
        assert!(!safe("rm -rf demo"));
        assert!(!safe("cat README.md > copy.md"));
        assert!(!safe("ls | cat"));
        assert!(!safe("echo $(pwd)"));
        assert!(!safe("sed -i '' 's/a/b/' file.txt"));
        assert!(!safe("find . -delete"));
        assert!(!safe("git checkout main"));
    }
}
