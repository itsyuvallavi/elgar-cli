//! Conversation pane state and rendering support.
//!
//! This file stores visible conversation lines, raw hidden details, scrollback,
//! and provider loading state.

mod reasoning_visibility;
mod scrollback;
mod style;

use elgar_core::{
    event::{AssistantMessageSource, Event, ProviderFinished},
    token_accounting::ProviderTokenUsage,
};

use crate::markdown::{assistant_markdown_has_hidden_details, render_assistant_markdown_details};

use super::{
    event_rendering::{render_tui_event, render_turn_metrics_summary},
    provider_reasoning::render_provider_reasoning,
};

use reasoning_visibility::provider_reasoning_should_stay_hidden;
use scrollback::ConversationScrollback;
pub(super) use scrollback::ThinkingPulse;
pub(crate) use style::ConversationLineStyle;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
    line_styles: Vec<ConversationLineStyle>,
    scrollback: ConversationScrollback,
    loading_pulse: ThinkingPulse,
    raw_details: Vec<String>,
    hidden_provider_reasoning_request_ids: Vec<String>,
}

impl ConversationPane {
    /// Apply one core event to the visible conversation pane.
    pub fn push_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(started) => {
                self.loading_pulse.reset();
                if provider_reasoning_should_stay_hidden(started) {
                    self.hidden_provider_reasoning_request_ids
                        .push(started.request_id.clone());
                }
            }
            Event::ProviderFinished(finished) => {
                self.remove_loading_pulse();
                if self.should_hide_provider_reasoning(finished) {
                    return;
                }
            }
            Event::Error(_) => self.remove_loading_pulse(),
            _ => {}
        }

        if let Event::AssistantMessage(message) = event {
            if message.source == AssistantMessageSource::Provider
                && assistant_markdown_has_hidden_details(&message.content)
            {
                self.raw_details
                    .push(render_assistant_markdown_details(&message.content));
            }
        }

        if let Event::ProviderFinished(finished) = event {
            self.push_provider_finished(finished);
            return;
        }

        if let Some((line, style)) = render_tui_event(event) {
            self.push_line(line, style);
        }
    }

    pub(crate) fn follow_latest(&mut self) {
        self.scrollback.follow_latest();
    }

    fn remove_loading_pulse(&mut self) {
        if self.last_line_style() == Some(ConversationLineStyle::Loading) {
            self.pop_line();
        }
    }

    pub fn push_local_message(&mut self, message: impl Into<String>) {
        self.push_line(message.into(), ConversationLineStyle::Plain);
    }

    pub(crate) fn clear(&mut self) {
        self.lines.clear();
        self.line_styles.clear();
        self.scrollback.follow_latest();
        self.loading_pulse.reset();
        self.raw_details.clear();
        self.hidden_provider_reasoning_request_ids.clear();
    }

    pub(crate) fn scroll_offset_for_lines(
        &self,
        content_lines: usize,
        viewport_height: u16,
    ) -> u16 {
        self.scrollback
            .offset_for(content_lines, usize::from(viewport_height))
    }

    pub(crate) fn render_body(&self) -> String {
        if self.lines.is_empty() {
            "(empty conversation)".to_string()
        } else {
            self.lines.join("\n")
        }
    }

    pub(crate) fn render_copy_body(&self) -> String {
        let lines = self
            .lines
            .iter()
            .enumerate()
            .filter(|(index, _line)| self.line_style(*index) != ConversationLineStyle::Thinking)
            .map(|(_index, line)| line.as_str())
            .collect::<Vec<_>>();

        if lines.is_empty() {
            "(empty conversation)".to_string()
        } else {
            lines.join("\n")
        }
    }

    pub(crate) fn render_raw_copy_body(&self) -> Option<String> {
        (!self.raw_details.is_empty()).then(|| self.raw_details.join("\n\n---\n\n"))
    }

    pub(crate) fn latest_raw_details(&self) -> Option<&str> {
        self.raw_details.last().map(String::as_str)
    }

    pub(crate) fn push_latest_raw_details(&mut self) -> bool {
        let Some(details) = self.latest_raw_details().map(str::to_string) else {
            return false;
        };

        self.push_line(details, ConversationLineStyle::Details);
        true
    }

    pub(crate) fn render_lines_with_styles(&self) -> Vec<(String, ConversationLineStyle)> {
        if self.lines.is_empty() {
            return vec![(
                "(empty conversation)".to_string(),
                ConversationLineStyle::Plain,
            )];
        }

        self.lines
            .iter()
            .enumerate()
            .flat_map(|(index, entry)| {
                let style = self.line_style(index);
                entry
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(move |line| (line.to_string(), style))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub(super) fn push_line(&mut self, line: String, style: ConversationLineStyle) {
        self.align_line_styles();
        self.lines.push(line);
        self.line_styles.push(style);
    }

    fn pop_line(&mut self) -> Option<String> {
        let line = self.lines.pop();
        if self.line_styles.len() > self.lines.len() {
            self.line_styles.pop();
        }
        line
    }

    fn last_line_style(&self) -> Option<ConversationLineStyle> {
        self.lines
            .len()
            .checked_sub(1)
            .map(|index| self.line_style(index))
    }

    fn line_style(&self, index: usize) -> ConversationLineStyle {
        self.line_styles
            .get(index)
            .copied()
            .unwrap_or(ConversationLineStyle::Plain)
    }

    fn align_line_styles(&mut self) {
        while self.line_styles.len() < self.lines.len() {
            self.line_styles.push(ConversationLineStyle::Plain);
        }
    }

    /// Render provider completion, reasoning, and usage summary.
    fn push_provider_finished(&mut self, finished: &ProviderFinished) {
        if let Some(line) = render_provider_reasoning(finished.output.thinking.as_deref()) {
            self.push_line(line, ConversationLineStyle::Thinking);
        }
    }

    pub(crate) fn push_turn_metrics(
        &mut self,
        total_duration_millis: u64,
        usage: Option<&ProviderTokenUsage>,
    ) {
        if let Some(line) = render_turn_metrics_summary(total_duration_millis, usage) {
            self.push_line(line, ConversationLineStyle::Metrics);
        }
    }

    fn should_hide_provider_reasoning(&mut self, finished: &ProviderFinished) -> bool {
        let Some(index) = self
            .hidden_provider_reasoning_request_ids
            .iter()
            .position(|request_id| request_id == &finished.request_id)
        else {
            return false;
        };

        self.hidden_provider_reasoning_request_ids.remove(index);
        true
    }
}
