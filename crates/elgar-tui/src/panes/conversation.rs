use elgar_core::event::{Event, ProviderFinished, ProviderTokenUsage};

use super::{
    event_rendering::{is_hidden_policy_approval, render_tui_event, render_turn_metrics_summary},
    provider_thinking::render_provider_thinking,
    tool_activity::{create_write_tool_item, CreateWriteToolBatch, CreateWriteToolItem},
};

#[cfg(test)]
use super::event_rendering::render_user_message;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
    pub(super) line_styles: Vec<ConversationLineStyle>,
    pub(super) scrollback: ConversationScrollback,
    pub(super) loading_pulse: ThinkingPulse,
    pub(super) create_batch: Option<CreateWriteToolBatch>,
}

impl ConversationPane {
    pub fn push_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(_) => self.loading_pulse.reset(),
            Event::ProviderFinished(_) | Event::Error(_) => self.remove_loading_pulse(),
            _ => {}
        }

        if let Event::ActionApplied(applied) = event {
            if let Some(item) = create_write_tool_item(&applied.result) {
                self.push_create_batch_item(item);
                return;
            }
        }

        if is_hidden_policy_approval(event) {
            return;
        }

        if !matches!(event, Event::Error(_)) {
            self.create_batch = None;
        }

        if let Event::ProviderFinished(finished) = event {
            self.push_provider_finished(finished);
            return;
        }

        if let Some((line, style)) = render_tui_event(event) {
            self.push_line(line, style);
        }
    }

    #[cfg(test)]
    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.scrollback.scroll_up(lines);
    }

    #[cfg(test)]
    pub(crate) fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    pub(crate) fn follow_latest(&mut self) {
        self.scrollback.follow_latest();
    }

    #[cfg(test)]
    pub(crate) fn is_following_latest(&self) -> bool {
        self.scrollback.is_following_latest()
    }

    #[cfg(test)]
    pub(crate) fn advance_loading_pulse(&mut self) {
        if self.last_line_style() == Some(ConversationLineStyle::Loading) {
            self.loading_pulse.advance();
            if let Some(last_line) = self.lines.last_mut() {
                *last_line = self.loading_pulse.label().to_string();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_pending_provider_turn(&mut self, content: &str) {
        self.loading_pulse.reset();
        self.push_line(render_user_message(content), ConversationLineStyle::User);
        self.push_line(
            self.loading_pulse.label().to_string(),
            ConversationLineStyle::Loading,
        );
    }

    #[cfg(test)]
    pub(crate) fn discard_pending_provider_turn(&mut self) {
        self.remove_loading_pulse();
        if self.last_line_style() == Some(ConversationLineStyle::User) {
            self.pop_line();
        }
    }

    fn remove_loading_pulse(&mut self) {
        if self.last_line_style() == Some(ConversationLineStyle::Loading) {
            self.pop_line();
        }
    }

    pub fn push_local_message(&mut self, message: impl Into<String>) {
        self.push_line(message.into(), ConversationLineStyle::Plain);
    }

    #[cfg(test)]
    pub(crate) fn scroll_offset(&self, viewport_height: u16) -> u16 {
        self.scroll_offset_for_lines(self.render_line_count(), viewport_height)
    }

    pub(crate) fn scroll_offset_for_lines(
        &self,
        content_lines: usize,
        viewport_height: u16,
    ) -> u16 {
        self.scrollback
            .offset_for(content_lines, usize::from(viewport_height))
    }

    #[cfg(test)]
    fn render_line_count(&self) -> usize {
        self.render_body().lines().count().max(1)
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

    fn push_create_batch_item(&mut self, item: CreateWriteToolItem) {
        match &mut self.create_batch {
            Some(batch) => {
                batch.push(item);
                if let Some(line) = self.lines.get_mut(batch.line_index) {
                    *line = batch.render();
                }
                if let Some(style) = self.line_styles.get_mut(batch.line_index) {
                    *style = batch.line_style();
                }
            }
            None => {
                let line_index = self.lines.len();
                let batch = CreateWriteToolBatch::new(line_index, item);
                let line = batch.render();
                let style = batch.line_style();
                self.create_batch = Some(batch);
                self.push_line(line, style);
            }
        }
    }

    fn push_provider_finished(&mut self, finished: &ProviderFinished) {
        if !finished.output.tool_calls.is_empty() {
            return;
        }

        if let Some(line) = render_provider_thinking(finished.output.thinking.as_deref()) {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ConversationLineStyle {
    #[default]
    Plain,
    Model,
    User,
    Loading,
    Thinking,
    Metrics,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ConversationScrollback {
    lines_from_bottom: usize,
}

impl ConversationScrollback {
    #[cfg(test)]
    fn scroll_up(&mut self, lines: usize) {
        self.lines_from_bottom = self.lines_from_bottom.saturating_add(lines);
    }

    #[cfg(test)]
    fn scroll_down(&mut self, lines: usize) {
        self.lines_from_bottom = self.lines_from_bottom.saturating_sub(lines);
    }

    fn follow_latest(&mut self) {
        self.lines_from_bottom = 0;
    }

    #[cfg(test)]
    fn is_following_latest(&self) -> bool {
        self.lines_from_bottom == 0
    }

    fn offset_for(&self, content_lines: usize, viewport_lines: usize) -> u16 {
        let max_offset = content_lines.saturating_sub(viewport_lines.max(1));
        max_offset
            .saturating_sub(self.lines_from_bottom.min(max_offset))
            .min(usize::from(u16::MAX)) as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct ThinkingPulse {
    index: usize,
}

impl ThinkingPulse {
    const LABELS: [&'static str; 4] = ["◐ working", "◓ working", "◑ working", "◒ working"];

    pub(super) fn label(&self) -> &'static str {
        Self::LABELS[self.index]
    }

    #[cfg(test)]
    pub(super) fn advance(&mut self) {
        self.index = (self.index + 1) % Self::LABELS.len();
    }

    pub(super) fn reset(&mut self) {
        self.index = 0;
    }
}
