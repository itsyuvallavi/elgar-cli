//! Live provider output preview state for inline prompt rendering.

use elgar_core::{
    event::ProviderStreamChunkReceived, provider::ProviderStreamChunk,
    provider_visible_text_from_text_only_output,
};

use crate::{
    markdown::render_assistant_markdown,
    panes::provider_reasoning::format_provider_reasoning_summary,
};

use super::wrap::compact_streaming_text;

const LIVE_REASONING_SUMMARY_CHARS: usize = 1200;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LiveProviderOutput {
    reasoning: String,
    response: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LiveResponsePreviewStats {
    pub(crate) raw_response_chars: usize,
    pub(crate) rendered_preview_chars: usize,
    pub(crate) rendered_preview_lines: usize,
    pub(crate) has_preview: bool,
}

impl LiveProviderOutput {
    pub(crate) fn push_stream_chunk(&mut self, chunk: &ProviderStreamChunkReceived) {
        match &chunk.chunk {
            ProviderStreamChunk::Reasoning(value) => self.reasoning.push_str(value),
            ProviderStreamChunk::Text(value) => self.response.push_str(value),
            ProviderStreamChunk::ToolCallDelta(_) => {}
        }
    }

    pub(super) fn reasoning_summary(&self) -> Option<String> {
        compact_streaming_text(&self.reasoning)
            .and_then(|text| format_provider_reasoning_summary(&text, LIVE_REASONING_SUMMARY_CHARS))
    }

    pub(crate) fn response_preview(&self) -> Option<String> {
        let visible = provider_visible_text_from_text_only_output(self.response.clone())?;
        let rendered = render_assistant_markdown(&visible);
        if rendered.trim().is_empty() {
            None
        } else {
            Some(rendered)
        }
    }

    pub(crate) fn reasoning_chars(&self) -> usize {
        self.reasoning.chars().count()
    }

    pub(crate) fn response_preview_stats(&self) -> LiveResponsePreviewStats {
        let preview = self.response_preview();
        LiveResponsePreviewStats {
            raw_response_chars: self.response.chars().count(),
            rendered_preview_chars: preview
                .as_deref()
                .map(|text| text.chars().count())
                .unwrap_or(0),
            rendered_preview_lines: preview
                .as_deref()
                .map(|text| text.lines().count())
                .unwrap_or(0),
            has_preview: preview.is_some(),
        }
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::{event::ProviderStreamChunkReceived, provider::ProviderStreamChunk};

    use super::LiveProviderOutput;

    #[test]
    fn live_output_accumulates_reasoning_chunks() {
        let mut output = LiveProviderOutput::default();
        output.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Reasoning("Need answer.".to_string()),
        ));

        assert!(output
            .reasoning_summary()
            .is_some_and(|summary| summary.contains("Need answer")));
    }

    #[test]
    fn live_output_keeps_longer_reasoning_preview() {
        let mut output = LiveProviderOutput::default();
        output.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Reasoning("a ".repeat(300)),
        ));

        let summary = output.reasoning_summary().expect("reasoning summary");

        assert!(summary.chars().count() > 160);
    }

    #[test]
    fn live_output_exposes_safe_response_preview() {
        let mut output = LiveProviderOutput::default();
        output.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello there.".to_string()),
        ));

        assert!(output
            .response_preview()
            .is_some_and(|preview| preview.contains("Hello there.")));
    }

    #[test]
    fn live_output_hides_raw_tool_protocol_preview() {
        let mut output = LiveProviderOutput::default();
        output.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("to=filesystem.create_file code".to_string()),
        ));

        assert!(output.response_preview().is_none());
    }

    #[test]
    fn live_output_reports_preview_stats() {
        let mut output = LiveProviderOutput::default();
        output.push_stream_chunk(&ProviderStreamChunkReceived::new(
            "provider",
            "request-1",
            1,
            ProviderStreamChunk::Text("Hello\nthere.".to_string()),
        ));

        let stats = output.response_preview_stats();

        assert_eq!(stats.raw_response_chars, 12);
        assert!(stats.has_preview);
        assert!(stats.rendered_preview_chars >= 12);
        assert!(stats.rendered_preview_lines >= 2);
    }
}
