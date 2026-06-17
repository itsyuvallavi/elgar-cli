//! Compact display for verified execution results.
//!
//! Core still owns and logs the raw `VERIFIED_*` result. This module only
//! turns that raw proof into a calmer terminal line for the default view.

/// Render a compact line for a raw verified execution block.
pub(crate) fn render_execution_result(raw: &str) -> Option<String> {
    if raw.starts_with("VERIFIED_WRITE_EXECUTION") {
        let outcome = field(raw, "write_outcome").unwrap_or("written");
        return Some(format!(
            "Done · {} {outcome}",
            field(raw, "path").unwrap_or("file"),
        ));
    }

    if raw.starts_with("VERIFIED_EDIT_EXECUTION") {
        return Some(format!(
            "Done · {} edited",
            field(raw, "path").unwrap_or("file")
        ));
    }

    if raw.starts_with("VERIFIED_BATCH_EXECUTION") {
        return Some(format!(
            "Done · {} approved actions executed",
            field(raw, "steps").unwrap_or("batch")
        ));
    }

    if raw.starts_with("VERIFIED_BASH_EXECUTION") {
        let command = field(raw, "command").unwrap_or("command");
        let exit = field(raw, "exit_code").unwrap_or("?");
        if exit == "0" {
            return Some(format!("Done · `{command}` exited 0"));
        }
        return Some(format!("Failed · `{command}` exited {exit}"));
    }

    None
}

fn field<'a>(raw: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}: ");
    raw.lines()
        .find_map(|line| line.strip_prefix(&prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::render_execution_result;

    #[test]
    fn renders_write_result_compactly() {
        let rendered = render_execution_result(
            "VERIFIED_WRITE_EXECUTION\napproval_id: approval-1\npath: hello-world.md\nbytes_written: 103\nwrite_outcome: created\n",
        )
        .expect("write result");

        assert_eq!(rendered, "Done · hello-world.md created");
    }

    #[test]
    fn renders_unchanged_write_result_compactly() {
        let rendered = render_execution_result(
            "VERIFIED_WRITE_EXECUTION\napproval_id: approval-1\npath: hello-world.md\nbytes_written: 103\nwrite_outcome: unchanged\n",
        )
        .expect("write result");

        assert_eq!(rendered, "Done · hello-world.md unchanged");
    }

    #[test]
    fn renders_bash_failure_compactly() {
        let rendered = render_execution_result(
            "VERIFIED_BASH_EXECUTION\ncommand: npm run build\nexit_code: 1\nstdout:\nstderr:\n",
        )
        .expect("bash result");

        assert_eq!(rendered, "Failed · `npm run build` exited 1");
    }
}
