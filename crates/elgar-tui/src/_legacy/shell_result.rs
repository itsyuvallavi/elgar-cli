//! Legacy shell result rendering.
//!
//! Archived from the old shell/tool execution UI.

use elgar_core::event::ShellActionVerification;

use crate::code_blocks::{render_code_block, CodeBlockInput};
use crate::shell_listing::{render_shell_listing_summary, shell_listing_fingerprint};

pub(crate) fn render_shell_execution_summary(shell: &ShellActionVerification) -> String {
    if let Some(listing) =
        render_shell_listing_summary(shell, &render_exit_and_duration(shell), details_hint())
    {
        return listing;
    }

    if let Some(file_read) =
        render_shell_file_read_summary(shell, &render_exit_and_duration(shell), details_hint())
    {
        return file_read;
    }

    let mut lines = vec!["Tool result".to_string(), render_shell_status(shell)];
    if let Some(streams) = render_hidden_stream_summary(shell) {
        lines.push(streams);
    }
    lines.push(details_hint().to_string());
    lines.join("\n")
}

pub(crate) fn render_shell_execution_details(shell: &ShellActionVerification) -> String {
    let mut lines = Vec::new();
    lines.push("Shell result details".to_string());
    lines.push(format!("Command: {}", shell.command.trim()));
    lines.push(format!("Cwd: {}", shell.cwd));
    if shell.timed_out {
        lines.push("Timed out: yes".to_string());
    } else if let Some(exit_code) = shell.exit_code {
        lines.push("Timed out: no".to_string());
        lines.push(format!("Exit code: {exit_code}"));
    } else {
        lines.push("Timed out: no".to_string());
        lines.push("Exit code: unavailable".to_string());
    }
    lines.push(format!(
        "Elapsed: {}",
        format_duration(shell.elapsed_millis)
    ));

    if let Some(effect) = &shell.verified_effect {
        lines.push(format!("Verified effect: {effect}"));
    }

    lines.push(render_raw_stream(
        "stdout",
        &shell.stdout,
        shell.stdout_truncated,
    ));
    lines.push(render_raw_stream(
        "stderr",
        &shell.stderr,
        shell.stderr_truncated,
    ));

    lines.join("\n")
}

pub(crate) fn shell_execution_listing_fingerprint(
    shell: &ShellActionVerification,
) -> Option<String> {
    shell_listing_fingerprint(shell)
}

pub(crate) fn render_repeated_shell_listing_summary(shell: &ShellActionVerification) -> String {
    [
        "Tool result".to_string(),
        format!(
            "same listing as previous · {}",
            render_exit_and_duration(shell)
        ),
        details_hint().to_string(),
    ]
    .join("\n")
}

fn render_shell_status(shell: &ShellActionVerification) -> String {
    if shell.timed_out {
        return format!(
            "shell command timed out · {}",
            format_duration(shell.elapsed_millis)
        );
    }

    match shell.exit_code {
        Some(0) => format!(
            "shell command finished · {}",
            render_exit_and_duration(shell)
        ),
        Some(exit_code) => format!(
            "shell command failed · exit {exit_code} · {}",
            format_duration(shell.elapsed_millis)
        ),
        None => format!(
            "shell command finished · exit unavailable · {}",
            format_duration(shell.elapsed_millis)
        ),
    }
}

fn render_exit_and_duration(shell: &ShellActionVerification) -> String {
    let exit = shell
        .exit_code
        .map(|code| format!("exit {code}"))
        .unwrap_or_else(|| "exit unavailable".to_string());
    format!("{exit} · {}", format_duration(shell.elapsed_millis))
}

fn render_hidden_stream_summary(shell: &ShellActionVerification) -> Option<String> {
    let mut parts = Vec::new();
    if !shell.stdout.trim().is_empty() {
        parts.push(hidden_stream_label("stdout", shell.stdout_truncated));
    }
    if !shell.stderr.trim().is_empty() {
        parts.push(hidden_stream_label("stderr", shell.stderr_truncated));
    }
    if shell.verified_effect.is_some() {
        parts.push("verified effect recorded".to_string());
    }

    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn render_shell_file_read_summary(
    shell: &ShellActionVerification,
    exit_and_duration: &str,
    details_hint: &str,
) -> Option<String> {
    if shell.timed_out
        || shell.exit_code != Some(0)
        || shell.stdout.trim().is_empty()
        || !shell.stderr.trim().is_empty()
        || !stdout_is_displayable_text(&shell.stdout)
    {
        return None;
    }

    let path = file_read_command_path(&shell.command)?;
    let display_lines = shell
        .stdout
        .trim_end_matches('\n')
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    if display_lines.is_empty() {
        return None;
    }

    let rendered = render_code_block(CodeBlockInput::new(&path, display_lines.clone()));
    let mut lines = vec![format!(
        "Read file · {path} · {} · {exit_and_duration}",
        file_line_count_label(display_lines.len())
    )];
    lines.extend(rendered.lines);
    if shell.stdout_truncated {
        lines.push("stdout truncated; use /details last or /copy raw".to_string());
    } else {
        lines.push(details_hint.to_string());
    }
    Some(lines.join("\n"))
}

fn file_read_command_path(command: &str) -> Option<String> {
    let command = shell_read_display_segment(command);
    let mut tokens = split_shell_words(command)?;
    strip_trailing_shell_redirect_words(&mut tokens);
    if tokens.iter().any(|token| is_shell_control_token(token)) {
        return None;
    }

    let command_name = tokens.first()?.rsplit('/').next().unwrap_or(&tokens[0]);
    match command_name {
        "cat" | "bat" | "batcat" => single_file_operand(&tokens[1..]),
        "head" | "tail" => single_file_operand_skipping_value_options(&tokens[1..]),
        "sed" => sed_file_operand(&tokens[1..]),
        _ => None,
    }
}

fn shell_read_display_segment(command: &str) -> &str {
    command
        .split_once("||")
        .map(|(left, _)| left.trim())
        .unwrap_or(command.trim())
}

fn strip_trailing_shell_redirect_words(tokens: &mut Vec<String>) {
    while tokens.last().is_some_and(|token| {
        token == "2>/dev/null"
            || token == "2>&1"
            || token == "1>/dev/null"
            || token == ">/dev/null"
            || token.starts_with("2>")
            || token.starts_with("1>")
    }) {
        tokens.pop();
    }
}

fn single_file_operand(tokens: &[String]) -> Option<String> {
    let operands = tokens
        .iter()
        .filter(|token| token.as_str() != "--" && !token.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();
    if operands.len() == 1 {
        Some(operands[0].clone())
    } else {
        None
    }
}

fn single_file_operand_skipping_value_options(tokens: &[String]) -> Option<String> {
    let mut operands = Vec::new();
    let mut skip_next = false;
    for token in tokens {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            token.as_str(),
            "-n" | "-c" | "--lines" | "--bytes" | "--lines=" | "--bytes="
        ) {
            skip_next = true;
            continue;
        }
        if token.starts_with("--lines=") || token.starts_with("--bytes=") {
            continue;
        }
        if token == "--" {
            continue;
        }
        if token.starts_with('-') {
            continue;
        }
        operands.push(token.clone());
    }

    if operands.len() == 1 {
        Some(operands[0].clone())
    } else {
        None
    }
}

fn sed_file_operand(tokens: &[String]) -> Option<String> {
    tokens
        .iter()
        .rev()
        .find(|token| looks_like_file_read_path(token))
        .cloned()
}

fn looks_like_file_read_path(token: &str) -> bool {
    if token.is_empty() || token.starts_with('-') || token.contains('\0') {
        return false;
    }
    token.contains('/')
        || token.contains('.')
        || matches!(
            token,
            "Dockerfile" | "Gemfile" | "LICENSE" | "Makefile" | "README" | "Rakefile"
        )
}

fn split_shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for character in command.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if quote.is_none() && character.is_whitespace() {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(character);
    }

    if escaped || quote.is_some() {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    Some(words)
}

fn is_shell_control_token(token: &str) -> bool {
    matches!(token, "|" | "&&" | "||" | ";" | "&") || token.contains('>') || token.contains('<')
}

fn stdout_is_displayable_text(stdout: &str) -> bool {
    let mut total = 0usize;
    let mut control = 0usize;
    for character in stdout.chars() {
        total += 1;
        if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
            control += 1;
        }
    }
    total > 0 && control.saturating_mul(20) <= total
}

fn file_line_count_label(line_count: usize) -> String {
    if line_count == 1 {
        "1 line".to_string()
    } else {
        format!("{line_count} lines")
    }
}

fn hidden_stream_label(label: &str, was_truncated: bool) -> String {
    if was_truncated {
        format!("{label} hidden (truncated)")
    } else {
        format!("{label} hidden")
    }
}

fn render_raw_stream(label: &str, value: &str, was_truncated: bool) -> String {
    let suffix = if was_truncated { " (truncated)" } else { "" };
    if value.is_empty() {
        return format!("{label}{suffix}: (empty)");
    }
    format!("{label}{suffix}:\n{}", value.trim_end_matches('\n'))
}

fn details_hint() -> &'static str {
    "details: /details last or /copy raw"
}

fn format_duration(millis: u64) -> String {
    if millis < 1_000 {
        format!("{millis}ms")
    } else {
        format!("{:.1}s", millis as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{render_shell_execution_details, render_shell_execution_summary};
    use elgar_core::event::ShellActionVerification;

    #[test]
    fn shell_summary_hides_raw_stdout_for_generic_command() {
        let rendered = render_shell_execution_summary(&shell_result(
            "printf hello",
            "hello\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("Tool result"));
        assert!(rendered.contains("shell command finished · exit 0"));
        assert!(rendered.contains("stdout hidden"));
        assert!(rendered.contains("/details last"));
        assert!(!rendered.contains("Command:"));
        assert!(!rendered.contains("Cwd:"));
        assert!(!rendered.contains("stdout:"));
        assert!(!rendered.contains("hello"));
    }

    #[test]
    fn shell_summary_renders_find_output_as_project_tree() {
        let rendered = render_shell_execution_summary(&shell_result(
            "find . -maxdepth 3 -print",
            ".\n./app\n./app/page.tsx\n./Cargo.toml\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("listed files · 3 entries · exit 0"));
        assert!(rendered.contains("Project tree\n.\n  Cargo.toml\n  app/\n    page.tsx"));
        assert!(!rendered.contains("stdout:"));
        assert!(!rendered.contains("./app/page.tsx"));
    }

    #[test]
    fn shell_summary_renders_ls_recursive_output_as_project_tree() {
        let rendered = render_shell_execution_summary(&shell_result(
            "ls -R .",
            ".:\nCargo.toml\napp\n\n./app:\npage.tsx\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("listed files · 3 entries · exit 0"));
        assert!(rendered.contains("Project tree\n.\n  Cargo.toml\n  app/\n    page.tsx"));
        assert!(!rendered.contains(".:"));
    }

    #[test]
    fn shell_summary_drops_partial_listing_line_when_stdout_is_truncated() {
        let mut shell = shell_result(
            "find . -maxdepth 3 -print",
            ".\n./app\n./app/page.tsx\n./playgro",
            "",
            Some(0),
            false,
        );
        shell.stdout_truncated = true;

        let rendered = render_shell_execution_summary(&shell);

        assert!(rendered.contains("app/\n    page.tsx"));
        assert!(rendered.contains("stdout truncated; use /details last or /copy raw"));
        assert!(!rendered.contains("playgro"));
    }

    #[test]
    fn shell_summary_reports_failure_and_stderr_without_dumping_streams() {
        let rendered =
            render_shell_execution_summary(&shell_result("npm test", "", "boom\n", Some(1), false));

        assert!(rendered.contains("shell command failed · exit 1"));
        assert!(rendered.contains("stderr hidden"));
        assert!(!rendered.contains("boom"));
    }

    #[test]
    fn shell_summary_renders_cat_file_stdout_as_code_panel() {
        let rendered = render_shell_execution_summary(&shell_result(
            "cat tailwind.config.ts",
            "export default {\n  content: [\"./app/**/*.tsx\"],\n}\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("Read file · tailwind.config.ts · 3 lines · exit 0 · 12ms"));
        assert!(rendered.contains(" ╭─ code (ts) · tailwind.config.ts · 3 lines "));
        assert!(rendered.contains(" │ export default {"));
        assert!(rendered.contains(" │   content: [\"./app/**/*.tsx\"],"));
        assert!(rendered.contains("details: /details last or /copy raw"));
        assert!(!rendered.contains("Tool result"));
        assert!(!rendered.contains("stdout hidden"));
        assert!(!rendered.contains("stdout:"));
    }

    #[test]
    fn shell_summary_renders_cat_file_with_safe_fallback_as_code_panel() {
        let rendered = render_shell_execution_summary(&shell_result(
            "cat app/page.tsx 2>/dev/null || echo \"FILE_NOT_FOUND\"",
            "export default function Page() {\n  return <main />;\n}\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("Read file · app/page.tsx · 3 lines · exit 0 · 12ms"));
        assert!(rendered.contains(" ╭─ code (tsx) · app/page.tsx · 3 lines "));
        assert!(rendered.contains(" │ export default function Page() {"));
        assert!(!rendered.contains("stdout hidden"));
    }

    #[test]
    fn shell_summary_renders_sed_file_stdout_as_code_panel() {
        let rendered = render_shell_execution_summary(&shell_result(
            "sed -n '1,120p' app/page.tsx",
            "export default function Page() {\n  return <main />;\n}\n",
            "",
            Some(0),
            false,
        ));

        assert!(rendered.contains("Read file · app/page.tsx · 3 lines · exit 0 · 12ms"));
        assert!(rendered.contains(" ╭─ code (tsx) · app/page.tsx · 3 lines "));
        assert!(rendered.contains(" │ export default function Page() {"));
        assert!(!rendered.contains("stdout hidden"));
    }

    #[test]
    fn shell_details_keep_full_captured_streams_for_raw_copy() {
        let rendered = render_shell_execution_details(&shell_result(
            "printf hello",
            "hello\nworld\n",
            "warn\n",
            Some(0),
            false,
        ));

        assert!(rendered.contains("Command: printf hello"));
        assert!(rendered.contains("Cwd: /repo"));
        assert!(rendered.contains("stdout:\nhello\nworld"));
        assert!(rendered.contains("stderr:\nwarn"));
    }

    fn shell_result(
        command: &str,
        stdout: &str,
        stderr: &str,
        exit_code: Option<i32>,
        timed_out: bool,
    ) -> ShellActionVerification {
        ShellActionVerification {
            command: command.to_string(),
            cwd: "/repo".to_string(),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            stdout_truncated: false,
            stderr_truncated: false,
            exit_code,
            elapsed_millis: 12,
            timed_out,
            verified_effect: None,
        }
    }
}
