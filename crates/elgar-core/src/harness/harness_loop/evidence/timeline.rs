//! Compact verified action timeline rendering for harness loop context.
//!
//! The timeline is derived only from Rust-collected evidence. It helps later
//! provider rounds and final answers keep failures, fixes, and reruns visible
//! without adding framework-specific rules.

use std::collections::BTreeSet;

use crate::harness::harness_loop::state::types::Evidence;

const MAX_RECENT_ACTIONS: usize = 12;

/// Append a compact verified timeline to a native provider tool result.
pub(in crate::harness::harness_loop) fn append_verified_action_timeline(
    body: &str,
    evidence: &[Evidence],
) -> String {
    let timeline = render_verified_action_timeline(evidence);
    if timeline.is_empty() {
        return body.to_string();
    }

    format!("{body}\n{timeline}")
}

/// Return compact metadata about the currently renderable action timeline.
pub(in crate::harness::harness_loop) fn verified_action_timeline_stats(
    evidence: &[Evidence],
) -> Option<VerifiedActionTimelineStats> {
    let actions = collect_actions(evidence);
    if actions.is_empty() {
        return None;
    }

    Some(VerifiedActionTimelineStats {
        action_count: actions.len(),
        rendered_action_count: selected_action_indexes(&actions).len(),
        failed_command_count: actions
            .iter()
            .filter(|action| action.failed_command)
            .count(),
    })
}

/// Render a bounded timeline of verified side effects and command outcomes.
pub(in crate::harness::harness_loop) fn render_verified_action_timeline(
    evidence: &[Evidence],
) -> String {
    let actions = collect_actions(evidence);
    if actions.is_empty() {
        return String::new();
    }

    let selected = selected_action_indexes(&actions);
    let omitted = actions.len().saturating_sub(selected.len());
    let has_failed_command = actions.iter().any(|action| action.failed_command);

    let mut rendered = String::from(
        "VERIFIED_ACTION_TIMELINE\n\
         Use this when answering. If a command failed and later passed, mention both the failure and the recovery.\n",
    );
    if omitted > 0 {
        rendered.push_str(&format!("omitted_older_actions: {omitted}\n"));
    }
    if has_failed_command {
        rendered.push_str("contains_failed_command: true\n");
    }

    for index in selected {
        rendered.push_str("- ");
        rendered.push_str(&actions[index].summary);
        rendered.push('\n');
    }

    rendered
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct VerifiedActionTimelineStats {
    pub action_count: usize,
    pub rendered_action_count: usize,
    pub failed_command_count: usize,
}

fn collect_actions(evidence: &[Evidence]) -> Vec<TimelineAction> {
    evidence
        .iter()
        .filter_map(action_from_evidence)
        .collect::<Vec<_>>()
}

fn selected_action_indexes(actions: &[TimelineAction]) -> Vec<usize> {
    let mut selected = BTreeSet::new();
    for (index, action) in actions.iter().enumerate() {
        if action.failed_command {
            selected.insert(index);
        }
    }
    let start = actions.len().saturating_sub(MAX_RECENT_ACTIONS);
    for index in start..actions.len() {
        selected.insert(index);
    }
    selected.into_iter().collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TimelineAction {
    summary: String,
    failed_command: bool,
}

fn action_from_evidence(item: &Evidence) -> Option<TimelineAction> {
    if item.body.starts_with("VERIFIED_BASH_EXECUTION") {
        return bash_action(&item.body);
    }
    if item.body.starts_with("VERIFIED_WRITE_EXECUTION") {
        return write_action(&item.body);
    }
    if item.body.starts_with("VERIFIED_EDIT_EXECUTION") {
        return edit_action(&item.body);
    }
    if item.body.starts_with("VERIFIED_EXECUTION_ERROR") {
        return execution_error_action(item);
    }
    if item.body.starts_with("VERIFIED_NOOP") {
        return noop_action(&item.body);
    }
    None
}

fn bash_action(body: &str) -> Option<TimelineAction> {
    let command = field_value(body, "command")?;
    let exit_code = field_value(body, "exit_code").unwrap_or("unknown");
    let status = if exit_code == "0" { "passed" } else { "failed" };
    Some(TimelineAction {
        summary: format!("bash `{command}` exit_code={exit_code} ({status})"),
        failed_command: exit_code != "0",
    })
}

fn write_action(body: &str) -> Option<TimelineAction> {
    let path = field_value(body, "path")?;
    let bytes = field_value(body, "bytes_written").unwrap_or("unknown");
    Some(TimelineAction {
        summary: format!("write `{path}` bytes={bytes} (content written, not independently read)"),
        failed_command: false,
    })
}

fn edit_action(body: &str) -> Option<TimelineAction> {
    let path = field_value(body, "path")?;
    let replacements = field_value(body, "replacements").unwrap_or("unknown");
    Some(TimelineAction {
        summary: format!("edit `{path}` replacements={replacements}"),
        failed_command: false,
    })
}

fn execution_error_action(item: &Evidence) -> Option<TimelineAction> {
    let error = field_value(&item.body, "error")?;
    Some(TimelineAction {
        summary: format!("execution error for `{}`: {error}", item.label),
        failed_command: true,
    })
}

fn noop_action(body: &str) -> Option<TimelineAction> {
    let target = field_value(body, "tool_target")?;
    let reason = field_value(body, "reason").unwrap_or("no-op");
    Some(TimelineAction {
        summary: format!("noop `{target}` ({reason})"),
        failed_command: false,
    })
}

fn field_value<'a>(body: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}: ");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(label: &str, body: &str) -> Evidence {
        Evidence {
            label: label.to_string(),
            body: body.to_string(),
            bytes: body.len(),
            truncated: false,
        }
    }

    #[test]
    fn timeline_keeps_failed_command_and_recovery() {
        let items = vec![
            evidence(
                "write:src/app/layout.tsx",
                "VERIFIED_WRITE_EXECUTION\npath: src/app/layout.tsx\nbytes_written: 12\n",
            ),
            evidence(
                "bash:npm run build",
                "VERIFIED_BASH_EXECUTION\ncommand: npm run build\nexit_code: 1\n",
            ),
            evidence(
                "write:src/app/globals.css",
                "VERIFIED_WRITE_EXECUTION\npath: src/app/globals.css\nbytes_written: 14\n",
            ),
            evidence(
                "bash:npm run build",
                "VERIFIED_BASH_EXECUTION\ncommand: npm run build\nexit_code: 0\n",
            ),
        ];

        let rendered = render_verified_action_timeline(&items);

        assert!(rendered.contains("contains_failed_command: true"));
        assert!(rendered.contains("bash `npm run build` exit_code=1 (failed)"));
        assert!(rendered.contains("write `src/app/globals.css`"));
        assert!(rendered.contains("bash `npm run build` exit_code=0 (passed)"));
    }

    #[test]
    fn timeline_stats_count_actions_without_rendering_body() {
        let items = vec![
            evidence(
                "write:src/app/layout.tsx",
                "VERIFIED_WRITE_EXECUTION\npath: src/app/layout.tsx\nbytes_written: 12\n",
            ),
            evidence(
                "bash:npm run build",
                "VERIFIED_BASH_EXECUTION\ncommand: npm run build\nexit_code: 1\n",
            ),
            evidence("read:package.json", "VERIFIED_READ\npath: package.json\n"),
        ];

        let stats = verified_action_timeline_stats(&items).expect("timeline stats");

        assert_eq!(stats.action_count, 2);
        assert_eq!(stats.rendered_action_count, 2);
        assert_eq!(stats.failed_command_count, 1);
    }
}
