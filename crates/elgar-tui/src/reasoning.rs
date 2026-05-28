use elgar_core::session::{ReasoningTrace, Session};

pub fn render_session_reasoning(session: &Session) -> String {
    let Some(trace) = session.latest_reasoning_trace() else {
        return "Reasoning\n(none)".to_string();
    };

    render_reasoning_trace(trace)
}

fn render_reasoning_trace(trace: &ReasoningTrace) -> String {
    let mut lines = vec!["Reasoning".to_string()];
    lines.push(format!("input: {}", trace.user_input));
    if let Some(route) = trace.route.as_deref() {
        lines.push(format!("route: {route}"));
    }

    push_section(&mut lines, "provider planning", &trace.provider_planning);
    push_section(&mut lines, "model decisions", &trace.model_decisions);
    push_section(&mut lines, "runtime checks", &trace.runtime_checks);

    lines.join("\n")
}

fn push_section(lines: &mut Vec<String>, title: &str, items: &[String]) {
    if items.is_empty() {
        lines.push(format!("{title}: (none recorded)"));
        return;
    }

    lines.push(format!("{title}:"));
    for item in items {
        lines.push(format!("- {item}"));
    }
}

pub(crate) fn format_provider_reasoning_summary(text: &str, max_chars: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    Some(truncate_chars(text, max_chars))
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
