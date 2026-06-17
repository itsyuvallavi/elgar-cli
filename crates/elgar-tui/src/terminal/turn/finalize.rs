//! Finalization decisions for interactive provider turns.
//!
//! This keeps the visible streamed answer stable when the completed provider
//! message renders exactly the same as the live preview already on screen.

use elgar_core::{
    event::{AssistantMessageSource, Event},
    token_accounting::ProviderTokenUsage,
};

use crate::{
    panes::{event_rendering::render_assistant_output, ConversationLineStyle},
    terminal::ui::prompt::LiveProviderOutput,
    turn_metrics::aggregate_provider_token_usage,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FinalizeDecision {
    pub(super) preserve_live_preview: bool,
    pub(super) live_preview_chars: usize,
    pub(super) final_chars: usize,
    pub(super) assistant_message_index: Option<usize>,
    pub(super) usage: Option<ProviderTokenUsage>,
}

impl FinalizeDecision {
    pub(super) fn should_preserve(&self) -> bool {
        self.preserve_live_preview && self.assistant_message_index.is_some()
    }
}

pub(super) fn decide_finalization(
    events: &[Event],
    live_output: &LiveProviderOutput,
) -> FinalizeDecision {
    let live_preview = live_output.response_preview();
    let assistant = latest_provider_assistant(events);
    let final_rendered = assistant.map(|(_index, content)| render_assistant_output(content));
    let preserve_live_preview = live_preview
        .as_deref()
        .zip(final_rendered.as_deref())
        .is_some_and(|(live, final_text)| normalized(live) == normalized(final_text));

    FinalizeDecision {
        preserve_live_preview,
        live_preview_chars: live_preview
            .as_deref()
            .map(|text| text.chars().count())
            .unwrap_or(0),
        final_chars: final_rendered
            .as_deref()
            .map(|text| text.chars().count())
            .unwrap_or(0),
        assistant_message_index: assistant.map(|(index, _content)| index),
        usage: aggregate_provider_token_usage(events),
    }
}

pub(super) fn final_lines_after_preserved_preview(
    events: &[Event],
    decision: &FinalizeDecision,
    include_metrics: bool,
    total_duration_millis: u64,
) -> Vec<(String, ConversationLineStyle)> {
    let mut lines = Vec::new();
    for (index, event) in events.iter().enumerate() {
        if Some(index) == decision.assistant_message_index {
            continue;
        }
        if matches!(event, Event::UserMessage(_) | Event::ProviderStarted(_)) {
            continue;
        }
        if let Some((line, style)) = crate::panes::event_rendering::render_tui_event(event) {
            lines.push((line, style));
        }
    }
    if include_metrics {
        if let Some(line) = crate::panes::event_rendering::render_turn_metrics_summary(
            total_duration_millis,
            decision.usage.as_ref(),
        ) {
            lines.push((line, ConversationLineStyle::Metrics));
        }
    }
    lines
}

fn latest_provider_assistant(events: &[Event]) -> Option<(usize, &str)> {
    events
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, event)| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider =>
            {
                Some((index, message.content.as_str()))
            }
            _ => None,
        })
}

fn normalized(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use elgar_core::{
        event::{AssistantMessage, AssistantMessageSource, Event, ProviderStreamChunkReceived},
        provider::ProviderStreamChunk,
    };

    use super::{decide_finalization, final_lines_after_preserved_preview};
    use crate::terminal::ui::prompt::LiveProviderOutput;

    #[test]
    fn preserves_live_preview_when_final_provider_text_matches() {
        let mut live = LiveProviderOutput::default();
        live.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello there.".to_string()),
        ));
        let events = vec![Event::AssistantMessage(AssistantMessage::new(
            "Hello there.",
            AssistantMessageSource::Provider,
        ))];

        let decision = decide_finalization(&events, &live);

        assert!(decision.should_preserve());
        assert_eq!(decision.assistant_message_index, Some(0));
    }

    #[test]
    fn falls_back_when_final_provider_text_differs() {
        let mut live = LiveProviderOutput::default();
        live.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello there.".to_string()),
        ));
        let events = vec![Event::AssistantMessage(AssistantMessage::new(
            "Different answer.",
            AssistantMessageSource::Provider,
        ))];

        let decision = decide_finalization(&events, &live);

        assert!(!decision.should_preserve());
    }

    #[test]
    fn preserved_preview_final_lines_skip_matching_assistant_message_and_metrics_by_default() {
        let mut live = LiveProviderOutput::default();
        live.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello there.".to_string()),
        ));
        let events = vec![Event::AssistantMessage(AssistantMessage::new(
            "Hello there.",
            AssistantMessageSource::Provider,
        ))];
        let decision = decide_finalization(&events, &live);

        let lines = final_lines_after_preserved_preview(&events, &decision, false, 1200);

        assert!(lines
            .iter()
            .all(|(line, _style)| !line.contains("Hello there")));
        assert!(lines.iter().all(|(line, _style)| !line.contains("1.2s")));
    }

    #[test]
    fn preserved_preview_can_include_metrics_when_requested() {
        let mut live = LiveProviderOutput::default();
        live.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello there.".to_string()),
        ));
        let events = vec![Event::AssistantMessage(AssistantMessage::new(
            "Hello there.",
            AssistantMessageSource::Provider,
        ))];
        let decision = decide_finalization(&events, &live);

        let lines = final_lines_after_preserved_preview(&events, &decision, true, 1200);

        assert!(lines.iter().any(|(line, _style)| line.contains("1.2s")));
    }
}
