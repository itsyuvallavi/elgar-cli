//! Live provider output preview state for inline prompt rendering.

use elgar_core::provider_visible_text_from_text_only_output;

use crate::{
    markdown::render_assistant_markdown,
    panes::provider_reasoning::format_provider_reasoning_summary,
};

use super::wrap::compact_streaming_text;

const LIVE_REASONING_SUMMARY_CHARS: usize = 160;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LiveProviderOutput {
    reasoning: String,
    response: String,
    suppress_response_preview: bool,
}

impl LiveProviderOutput {
    pub(crate) fn suppress_response_preview(&mut self) {
        self.suppress_response_preview = true;
    }

    pub(super) fn reasoning_summary(&self) -> Option<String> {
        compact_streaming_text(&self.reasoning)
            .and_then(|text| format_provider_reasoning_summary(&text, LIVE_REASONING_SUMMARY_CHARS))
    }

    pub(super) fn response_preview(&self) -> Option<String> {
        if self.suppress_response_preview {
            return None;
        }

        let visible = provider_visible_text_from_text_only_output(self.response.clone())?;
        let rendered = render_assistant_markdown(&visible);
        if rendered.trim().is_empty() {
            None
        } else {
            Some(rendered)
        }
    }
}
