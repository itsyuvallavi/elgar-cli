//! Legacy shell listing summary rendering.
//!
//! Archived from the old shell/tool execution UI.

use std::collections::BTreeMap;

use elgar_core::event::ShellActionVerification;

const MAX_TREE_LINES: usize = 80;
const MAX_TREE_LINE_CHARS: usize = 96;

pub(crate) fn render_shell_listing_summary(
    shell: &ShellActionVerification,
    exit_and_duration: &str,
    details_hint: &str,
) -> Option<String> {
    if shell.timed_out || shell.exit_code != Some(0) {
        return None;
    }

    let listing_kind = listing_command_kind(&shell.command)?;
    let entries = listing_entries(listing_kind, &shell.stdout, shell.stdout_truncated);
    if entries.is_empty() {
        return None;
    }

    let mut lines = vec![
        "Tool result".to_string(),
        format!(
            "listed files · {} · {exit_and_duration}",
            pluralize(entries.len(), "entry", "entries")
        ),
        "Project tree".to_string(),
    ];
    lines.extend(render_path_tree(&entries));

    if shell.stdout_truncated {
        lines.push("stdout truncated; use /details last or /copy raw".to_string());
    } else {
        lines.push(details_hint.to_string());
    }

    Some(lines.join("\n"))
}

pub(crate) fn shell_listing_fingerprint(shell: &ShellActionVerification) -> Option<String> {
    if shell.timed_out || shell.exit_code != Some(0) {
        return None;
    }

    let listing_kind = listing_command_kind(&shell.command)?;
    let mut entries = listing_entries(listing_kind, &shell.stdout, shell.stdout_truncated);
    entries.sort();
    (!entries.is_empty()).then(|| entries.join("\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListingCommandKind {
    PathLines,
    LsRecursive,
    LsFlat,
}

fn listing_command_kind(command: &str) -> Option<ListingCommandKind> {
    let lower = command.trim().to_ascii_lowercase();
    let tokens = lower.split_whitespace().collect::<Vec<_>>();
    let first = tokens.first().copied()?;
    let command_name = first.rsplit('/').next().unwrap_or(first);

    if command_name == "find" || lower.starts_with("rg --files") || lower.starts_with("fd ") {
        return Some(ListingCommandKind::PathLines);
    }

    if lower.starts_with("git ls-files") {
        return Some(ListingCommandKind::PathLines);
    }

    if command_name == "ls" {
        if tokens
            .iter()
            .skip(1)
            .any(|token| token.starts_with('-') && token.chars().any(|character| character == 'r'))
        {
            return Some(ListingCommandKind::LsRecursive);
        }
        return Some(ListingCommandKind::LsFlat);
    }

    None
}

fn listing_entries(kind: ListingCommandKind, stdout: &str, stdout_truncated: bool) -> Vec<String> {
    let lines = complete_listing_lines(stdout, stdout_truncated);
    match kind {
        ListingCommandKind::PathLines => path_line_entries(&lines),
        ListingCommandKind::LsRecursive => ls_recursive_entries(&lines),
        ListingCommandKind::LsFlat => ls_flat_entries(&lines),
    }
}

fn complete_listing_lines(stdout: &str, stdout_truncated: bool) -> Vec<&str> {
    let mut lines = stdout.lines().collect::<Vec<_>>();
    if stdout_truncated && !stdout.ends_with('\n') {
        lines.pop();
    }
    lines
}

fn path_line_entries(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| clean_listing_path(line))
        .collect()
}

fn ls_recursive_entries(lines: &[&str]) -> Vec<String> {
    let mut entries = Vec::new();
    let mut current_dir = String::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }

        if let Some(directory) = trimmed.strip_suffix(':') {
            current_dir = clean_listing_path(directory).unwrap_or_default();
            continue;
        }

        let Some(name) = ls_entry_name(trimmed) else {
            continue;
        };
        let path = if current_dir.is_empty() {
            name
        } else {
            format!("{current_dir}/{name}")
        };
        if let Some(cleaned) = clean_listing_path(&path) {
            entries.push(cleaned);
        }
    }

    entries
}

fn ls_flat_entries(lines: &[&str]) -> Vec<String> {
    let mut entries = Vec::new();

    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }

        if let Some(name) = ls_entry_name(trimmed) {
            if let Some(cleaned) = clean_listing_path(&name) {
                entries.push(cleaned);
            }
            continue;
        }

        for name in trimmed.split_whitespace() {
            if let Some(cleaned) = clean_listing_path(name) {
                entries.push(cleaned);
            }
        }
    }

    entries
}

fn ls_entry_name(line: &str) -> Option<String> {
    let first = line.chars().next()?;
    if !matches!(first, '-' | 'd' | 'l') {
        return Some(line.to_string());
    }

    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 9 {
        return None;
    }

    let name = parts[8..].join(" ");
    Some(
        name.split_once(" -> ")
            .map(|(target, _link)| target.to_string())
            .unwrap_or(name),
    )
}

fn clean_listing_path(value: &str) -> Option<String> {
    let mut path = value.trim();
    if path.is_empty() || matches!(path, "." | "./") {
        return None;
    }
    while let Some(stripped) = path.strip_prefix("./") {
        path = stripped;
    }
    path = path.trim_end_matches('/');
    if path.is_empty() || path.contains('\0') {
        None
    } else {
        Some(path.to_string())
    }
}

#[derive(Debug, Default)]
struct TreeNode {
    children: BTreeMap<String, TreeNode>,
}

fn render_path_tree(entries: &[String]) -> Vec<String> {
    let mut root = TreeNode::default();
    for entry in entries {
        insert_tree_path(&mut root, entry);
    }

    let mut lines = vec![".".to_string()];
    let mut rendered_entries = 0usize;
    render_tree_children(&root, 1, &mut lines, &mut rendered_entries);
    if rendered_entries < entries.len() {
        lines.push(format!(
            "... {} more",
            pluralize(entries.len() - rendered_entries, "entry", "entries")
        ));
    }
    lines
}

fn insert_tree_path(root: &mut TreeNode, path: &str) {
    let mut node = root;
    for component in path.split('/').filter(|component| !component.is_empty()) {
        node = node.children.entry(component.to_string()).or_default();
    }
}

fn render_tree_children(
    node: &TreeNode,
    depth: usize,
    lines: &mut Vec<String>,
    rendered_entries: &mut usize,
) {
    for (name, child) in &node.children {
        if *rendered_entries >= MAX_TREE_LINES {
            return;
        }

        let suffix = if child.children.is_empty() { "" } else { "/" };
        let indent = "  ".repeat(depth);
        lines.push(compact_tree_line(&format!("{indent}{name}{suffix}")));
        *rendered_entries += 1;

        render_tree_children(child, depth + 1, lines, rendered_entries);
    }
}

fn compact_tree_line(line: &str) -> String {
    if line.chars().count() <= MAX_TREE_LINE_CHARS {
        return line.to_string();
    }

    let mut compact = line
        .chars()
        .take(MAX_TREE_LINE_CHARS.saturating_sub(3))
        .collect::<String>();
    compact.push_str("...");
    compact
}

fn pluralize(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}
