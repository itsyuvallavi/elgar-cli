use elgar_core::event::ShellActionVerification;

pub(crate) fn render_shell_execution_details(shell: &ShellActionVerification) -> String {
    let mut lines = Vec::new();
    if shell.timed_out {
        lines.push(format!(
            "Shell command timed out after {} ms.",
            shell.elapsed_millis
        ));
    } else if let Some(exit_code) = shell.exit_code {
        lines.push(format!("Shell command finished: exit {exit_code}."));
    } else {
        lines.push("Shell command finished; exit code unavailable.".to_string());
    }

    if let Some(stdout) = render_shell_stream("stdout", &shell.stdout, shell.stdout_truncated) {
        lines.push(stdout);
    }
    if let Some(stderr) = render_shell_stream("stderr", &shell.stderr, shell.stderr_truncated) {
        lines.push(stderr);
    }

    lines.join("\n")
}

fn render_shell_stream(label: &str, value: &str, was_truncated: bool) -> Option<String> {
    let compact = compact_shell_stream(value)?;
    let suffix = if was_truncated { " (truncated)" } else { "" };
    Some(format!("{label}: {compact}{suffix}"))
}

fn compact_shell_stream(value: &str) -> Option<String> {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return None;
    }

    const LIMIT: usize = 240;
    if line.len() <= LIMIT {
        return Some(line);
    }

    let suffix = "...";
    let mut end = LIMIT.saturating_sub(suffix.len()).min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    Some(format!("{}{}", &line[..end], suffix))
}
