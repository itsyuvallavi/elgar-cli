//! Compact verified event rendering.
//!
//! These helpers are display-only. They turn exact harness evidence labels and
//! raw verified execution blocks into concise terminal rows without changing
//! tool execution, approval, or retry behavior.

use super::execution_result::render_execution_result;

/// Render a compact event row for exact harness evidence or execution proof.
pub(crate) fn render_event_block(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(result) = render_execution_result(trimmed) {
        return Some(result);
    }

    let rows = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(render_evidence_label)
        .collect::<Option<Vec<_>>>()?;

    (!rows.is_empty()).then(|| rows.join("\n"))
}

fn render_evidence_label(label: &str) -> Option<String> {
    if let Some(fingerprint) = label.strip_prefix("invalid_mcp_call:") {
        return Some(format!(
            "MCP · invalid call shape · {}",
            short_hash(fingerprint)
        ));
    }

    if let Some(rest) = label.strip_prefix("mcp:") {
        let mut parts = rest.splitn(3, ':');
        let server = parts.next()?;
        let tool = parts.next()?;
        let fingerprint = parts.next()?;
        return Some(format!(
            "MCP · {server}/{tool} verified · {}",
            short_hash(fingerprint)
        ));
    }

    if let Some(path) = label.strip_prefix("read:") {
        return Some(format!("Tool · read `{path}`"));
    }

    if let Some(path) = label.strip_prefix("ls:") {
        return Some(format!("Tool · list `{path}`"));
    }

    if let Some(rest) = label.strip_prefix("find:") {
        let (path, pattern) = rest.split_once(':')?;
        return Some(format!("Tool · find `{pattern}` under `{path}`"));
    }

    if let Some(rest) = label.strip_prefix("grep:") {
        let (path, query) = rest.split_once(':')?;
        return Some(format!("Tool · search `{query}` in `{path}`"));
    }

    if let Some(rest) = label.strip_prefix("write:") {
        let target = label_target(rest)?;
        return Some(format!("Write · `{target}` verified"));
    }

    if let Some(rest) = label.strip_prefix("edit:") {
        let target = label_target(rest)?;
        return Some(format!("Edit · `{target}` verified"));
    }

    if let Some(rest) = label.strip_prefix("bash:") {
        let target = label_target(rest)?;
        return Some(format!("Command · `{target}` verified"));
    }

    None
}

fn label_target(rest: &str) -> Option<&str> {
    rest.rsplit_once(':')
        .map(|(target, _suffix)| target)
        .filter(|target| !target.is_empty())
}

fn short_hash(value: &str) -> &str {
    value.get(..8).unwrap_or(value)
}

#[cfg(test)]
mod tests {
    use super::render_event_block;

    #[test]
    fn renders_mcp_label_compactly() {
        let rendered =
            render_event_block("mcp:context7:query-docs:4da7ba409202bb3e").expect("mcp label");

        assert_eq!(rendered, "MCP · context7/query-docs verified · 4da7ba40");
    }

    #[test]
    fn renders_local_tool_labels_compactly() {
        let rendered =
            render_event_block("read:package.json\ngrep:app:export").expect("tool labels");

        assert_eq!(
            rendered,
            "Tool · read `package.json`\nTool · search `export` in `app`"
        );
    }

    #[test]
    fn renders_verified_execution_result() {
        let rendered = render_event_block(
            "VERIFIED_WRITE_EXECUTION\npath: hello-world.md\nwrite_outcome: created\n",
        )
        .expect("write result");

        assert_eq!(rendered, "Done · hello-world.md created");
    }

    #[test]
    fn ignores_normal_prose() {
        assert_eq!(render_event_block("Here is the summary."), None);
    }
}
