use crate::{agent_visibility::looks_like_raw_tool_protocol, session::Session};

pub(crate) fn record_provider_planning_trace(
    session: &mut Session,
    thinking: Option<&str>,
    assistant_text: &str,
) {
    if let Some(thinking) = thinking.filter(|value| !value.trim().is_empty()) {
        session.push_reasoning_provider_planning(format!("thinking: {}", thinking.trim()));
    }

    let text = assistant_text.trim();
    if !text.is_empty() && !looks_like_raw_tool_protocol(text) {
        session.push_reasoning_provider_planning(format!("visible text: {text}"));
    }
}
