use elgar_core::event::{AssistantMessageSource, Event, VerifiedActionResult};

use crate::markdown::render_assistant_markdown;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
    line_styles: Vec<ConversationLineStyle>,
    scrollback: ConversationScrollback,
    loading_pulse: ThinkingPulse,
}

impl ConversationPane {
    pub fn push_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(_) => self.loading_pulse.reset(),
            Event::ProviderFinished(_) | Event::Error(_) => self.remove_loading_pulse(),
            _ => {}
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
                    .map(move |line| (line.to_string(), style))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn push_line(&mut self, line: String, style: ConversationLineStyle) {
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ConversationLineStyle {
    #[default]
    Plain,
    User,
    Loading,
    Thinking,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ConversationScrollback {
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

    fn offset_for(&self, content_lines: usize, viewport_lines: usize) -> u16 {
        let max_offset = content_lines.saturating_sub(viewport_lines.max(1));
        max_offset
            .saturating_sub(self.lines_from_bottom.min(max_offset))
            .min(usize::from(u16::MAX)) as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputArea {
    pub text: String,
}

impl InputArea {
    pub(crate) fn render_body(&self) -> String {
        format!("> {}", self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyArea {
    last_result: Option<CopyResult>,
}

impl CopyArea {
    pub(crate) fn mark_copied(&mut self, bytes: usize) {
        self.last_result = Some(CopyResult::Copied { bytes });
    }

    pub(crate) fn mark_failed(&mut self, message: impl Into<String>) {
        self.last_result = Some(CopyResult::Failed {
            message: message.into(),
        });
    }

    pub(crate) fn render_hint(&self) -> String {
        match &self.last_result {
            Some(CopyResult::Copied { bytes }) => {
                format!("copied conversation ({bytes} bytes)")
            }
            Some(CopyResult::Failed { message }) => {
                format!("copy failed: {message}")
            }
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CopyResult {
    Copied { bytes: usize },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    thinking_pulse: ThinkingPulse,
    provider_active: bool,
}

impl StatusLine {
    pub fn ready() -> Self {
        Self {
            text: "ready".to_string(),
            thinking_pulse: ThinkingPulse::default(),
            provider_active: false,
        }
    }

    pub fn observe_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(_) => self.start_thinking_pulse(),
            Event::ProviderFinished(_) => self.finish("reply ready"),
            Event::Error(error) => {
                if parse_provider_error(&error.message).is_some() {
                    self.finish("provider error");
                } else {
                    self.finish("error");
                }
            }
            _ => {
                self.provider_active = false;
                self.text = match event {
                    Event::UserMessage(_) => "sent".to_string(),
                    Event::AssistantMessage(_) => "reply ready".to_string(),
                    Event::ActionProposed(action) => {
                        format!("review {}", action.action_id)
                    }
                    Event::ActionApproved(action) => {
                        format!("approved {}", action.action_id)
                    }
                    Event::ActionRejected(action) => {
                        format!("rejected {}", action.action_id)
                    }
                    Event::ActionApplied(action) => {
                        format!("applied {}", action.action_id)
                    }
                    Event::ActionFailed(action) => {
                        format!("failed {}", action.action_id)
                    }
                    Event::ProviderStarted(_) | Event::ProviderFinished(_) | Event::Error(_) => {
                        unreachable!("provider and error events are handled above")
                    }
                };
            }
        }
    }

    pub(crate) fn start_thinking_pulse(&mut self) {
        self.provider_active = true;
        self.thinking_pulse.reset();
        self.text = self.thinking_pulse.label().to_string();
    }

    #[cfg(test)]
    pub(crate) fn cancel_provider_turn(&mut self) {
        self.finish("canceled");
    }

    #[cfg(test)]
    pub(crate) fn advance_thinking_pulse(&mut self) {
        if self.provider_active {
            self.thinking_pulse.advance();
            self.text = self.thinking_pulse.label().to_string();
        }
    }

    #[cfg(test)]
    pub(crate) fn provider_active(&self) -> bool {
        self.provider_active
    }

    pub(crate) fn render_body(&self) -> String {
        self.text.clone()
    }

    fn finish(&mut self, text: &'static str) {
        self.provider_active = false;
        self.text = text.to_string();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ThinkingPulse {
    index: usize,
}

impl ThinkingPulse {
    const LABELS: [&'static str; 4] = ["◐ working", "◓ working", "◑ working", "◒ working"];

    fn label(&self) -> &'static str {
        Self::LABELS[self.index]
    }

    #[cfg(test)]
    fn advance(&mut self) {
        self.index = (self.index + 1) % Self::LABELS.len();
    }

    fn reset(&mut self) {
        self.index = 0;
    }
}

fn render_tui_event(event: &Event) -> Option<(String, ConversationLineStyle)> {
    match event {
        Event::UserMessage(message) => Some((
            render_user_message(&message.content),
            ConversationLineStyle::User,
        )),
        Event::AssistantMessage(message) => {
            let rendered = match message.source {
                AssistantMessageSource::Controller => {
                    render_labeled_output("Elgar", &message.content)
                }
                AssistantMessageSource::Provider => render_assistant_output(&message.content),
            };
            Some((rendered, ConversationLineStyle::Plain))
        }
        Event::ProviderStarted(_) => {
            Some((render_thinking_progress(), ConversationLineStyle::Loading))
        }
        Event::ProviderFinished(finished) => {
            render_provider_thinking(finished.output.thinking.as_deref())
                .map(|line| (line, ConversationLineStyle::Thinking))
        }
        Event::ActionProposed(action) => Some(format!(
            "Review needed: {} {:?} {}",
            action.action_id, action.action_kind, action.summary
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::ActionApproved(action) => Some(format!(
            "Approved: {} {:?} {}",
            action.action_id, action.action_kind, action.summary
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::ActionRejected(action) => Some(format!(
            "Rejected: {} {:?} {}. No file was changed.",
            action.action_id, action.action_kind, action.summary
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::ActionApplied(applied) => Some(format!(
            "Applied and verified: {} {:?} {}",
            applied.action_id,
            applied.action_kind,
            render_verified_result(&applied.result)
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::ActionFailed(failed) => Some(format!(
            "Action failed: {} {:?} {}",
            failed.action_id, failed.action_kind, failed.reason
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::Error(error) => Some((
            render_error_line(&error.message),
            ConversationLineStyle::Plain,
        )),
    }
}

fn render_user_message(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_labeled_output(label: &str, content: &str) -> String {
    let rendered = render_assistant_output(content);
    if rendered.contains('\n') {
        format!("{label}:\n{rendered}")
    } else {
        format!("{label}: {rendered}")
    }
}

fn render_assistant_output(content: &str) -> String {
    render_assistant_markdown(content)
}

fn render_thinking_progress() -> String {
    ThinkingPulse::default().label().to_string()
}

fn render_provider_thinking(thinking: Option<&str>) -> Option<String> {
    let thinking = thinking?.trim();
    if thinking.is_empty() {
        return None;
    }

    Some(ThinkingBlock::collapsed(thinking).render_collapsed())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ThinkingBlock {
    summary: String,
    detail: String,
    expanded: bool,
}

impl ThinkingBlock {
    fn collapsed(detail: &str) -> Self {
        Self {
            summary: compact_thinking_summary(detail),
            detail: render_assistant_markdown(detail),
            expanded: false,
        }
    }

    fn render_collapsed(&self) -> String {
        let _future_expanded_detail = if self.expanded {
            Some(self.detail.as_str())
        } else {
            None
        };
        self.summary.clone()
    }
}

fn compact_thinking_summary(thinking: &str) -> String {
    let rendered = render_assistant_markdown(thinking);
    let summary = rendered.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 96;
    if summary.chars().count() <= MAX_CHARS {
        return summary;
    }

    let mut compact = summary.chars().take(MAX_CHARS - 1).collect::<String>();
    compact.push('…');
    compact
}

fn render_verified_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => format!("{path} was written"),
    }
}

fn render_error_line(message: &str) -> String {
    if let Some(provider_error) = parse_provider_error(message) {
        format!(
            "Provider error from {}: {}",
            provider_error.provider, provider_error.detail
        )
    } else {
        format!("Error: {message}")
    }
}

struct ProviderErrorParts<'a> {
    provider: &'a str,
    detail: &'a str,
}

fn parse_provider_error(message: &str) -> Option<ProviderErrorParts<'_>> {
    let (provider, rest) = message.split_once(" provider request ")?;
    let (_request_id, detail) = rest.split_once(" failed: ")?;
    Some(ProviderErrorParts { provider, detail })
}

#[cfg(test)]
mod tests {
    use elgar_core::event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderOutput, ProviderStarted, UserMessage,
        VerifiedActionResult,
    };

    use super::{ConversationPane, CopyArea, InputArea, StatusLine};

    #[test]
    fn conversation_displays_user_assistant_provider_action_and_error_output() {
        let mut conversation = ConversationPane::default();
        let events = vec![
            Event::UserMessage(UserMessage::new("hello")),
            Event::AssistantMessage(AssistantMessage::new(
                "hi",
                AssistantMessageSource::Controller,
            )),
            Event::ProviderStarted(ProviderStarted::new("stub-provider", "request-1")),
            Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("provider text"),
            )),
            Event::ActionProposed(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )),
            Event::ActionApproved(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )),
            Event::ActionApplied(ActionApplied::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                VerifiedActionResult::FileWritten {
                    path: "hello.py".to_string(),
                },
            )),
            Event::ActionRejected(ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::WriteFile,
                "write rejected.py",
            )),
            Event::ActionFailed(ActionFailed::new(
                "action-3",
                elgar_core::event::ActionKind::WriteFile,
                "permission denied",
            )),
            Event::Error(ErrorEvent::new("boom")),
        ];

        for event in &events {
            conversation.push_event(event);
        }

        let rendered = conversation.render_body();
        assert!(rendered.contains("> hello"));
        assert!(!rendered.contains("User\n"));
        assert!(rendered.contains("Elgar: hi"));
        assert!(!rendered.contains("thinking"));
        assert!(!rendered.contains("request-1"));
        assert!(!rendered.contains("Provider text is suggestion only."));
        assert!(rendered.contains("Review needed: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("Approved: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("Applied and verified: action-1 WriteFile hello.py was written"));
        assert!(rendered
            .contains("Rejected: action-2 WriteFile write rejected.py. No file was changed."));
        assert!(rendered.contains("Action failed: action-3 WriteFile permission denied"));
        assert!(rendered.contains("Error: boom"));
    }

    #[test]
    fn empty_panes_render_default_body_text() {
        assert_eq!(
            ConversationPane::default().render_body(),
            "(empty conversation)"
        );
        assert_eq!(InputArea::default().render_body(), "> ");
        assert_eq!(CopyArea::default().render_hint(), "");
    }

    #[test]
    fn copy_area_tracks_copy_result_without_changing_conversation() {
        let mut copy = CopyArea::default();

        copy.mark_copied(12);
        assert_eq!(copy.render_hint(), "copied conversation (12 bytes)");

        copy.mark_failed("terminal rejected OSC 52");
        assert_eq!(copy.render_hint(), "copy failed: terminal rejected OSC 52");
    }

    #[test]
    fn status_line_tracks_last_event_kind() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new("boom")));

        assert_eq!(status.text, "error");
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn conversation_renders_provider_errors_with_calm_copy() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));

        assert_eq!(
            conversation.render_body(),
            "Provider error from fake-provider: Provider provider error (404): model missing"
        );
    }

    #[test]
    fn conversation_renders_controller_errors_without_provider_label() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));

        assert_eq!(
            conversation.render_body(),
            "Error: Input was not recognized."
        );
    }

    #[test]
    fn conversation_renders_assistant_markdown_as_presentation_only_text() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Plan:\n- **read** files\n- `render` output\n\n```rust\nfn main() {}\n```",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("Plan:\n- read files\n- render output"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("code (rust):\n    fn main() {}"));
        assert!(!rendered.contains("```"));
        assert!(!rendered.contains("**read**"));
    }

    #[test]
    fn conversation_renders_assistant_markdown_tables_readably() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "| File | State |\n| --- | --- |\n| src/lib.rs | changed |",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("  File"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("changed"));
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn conversation_uses_pi_style_user_block_and_unlabeled_provider_reply() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::UserMessage(UserMessage::new(
            "explain this\nin two lines",
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "short answer",
            AssistantMessageSource::Provider,
        )));

        assert_eq!(
            conversation.render_body(),
            "> explain this\n> in two lines\nshort answer"
        );
    }

    #[test]
    fn conversation_pulses_loading_inside_transcript() {
        let mut conversation = ConversationPane::default();

        conversation.push_pending_provider_turn("hello");
        assert_eq!(conversation.render_body(), "> hello\n◐ working");

        conversation.advance_loading_pulse();
        assert_eq!(conversation.render_body(), "> hello\n◓ working");

        conversation.discard_pending_provider_turn();
        assert_eq!(conversation.render_body(), "(empty conversation)");
    }

    #[test]
    fn conversation_renders_explicit_provider_thinking_before_model_answer() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer")
                .with_thinking("Read the prompt.\nReturn concise text."),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        let thinking_index = rendered.find("Read the prompt.").unwrap();
        let model_index = rendered.find("final answer").unwrap();

        assert!(!rendered.contains("Thinking\n"));
        assert!(!rendered.contains("thinking:"));
        assert!(thinking_index < model_index);
        assert!(rendered.contains("Return concise text."));
        assert!(!rendered.contains("request-1"));
    }

    #[test]
    fn conversation_keeps_existing_progress_when_provider_thinking_is_absent() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer"),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("thinking"));
        assert!(rendered.contains("final answer"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("Thinking\nfinal answer"));
    }

    #[test]
    fn conversation_scrollback_computes_view_offset_without_changing_lines() {
        let mut conversation = ConversationPane {
            lines: (0..10).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };
        let original_lines = conversation.lines.clone();

        assert_eq!(conversation.scroll_offset(4), 6);

        conversation.scroll_up(2);
        assert_eq!(conversation.scroll_offset(4), 4);
        assert_eq!(conversation.lines, original_lines);

        conversation.scroll_down(1);
        assert_eq!(conversation.scroll_offset(4), 5);

        conversation.follow_latest();
        assert_eq!(conversation.scroll_offset(4), 6);
    }

    #[test]
    fn conversation_scrollback_clamps_to_available_content() {
        let mut conversation = ConversationPane {
            lines: (0..3).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };

        assert_eq!(conversation.scroll_offset(6), 0);

        conversation.scroll_up(100);
        assert_eq!(conversation.scroll_offset(2), 0);
    }

    #[test]
    fn status_line_distinguishes_provider_and_controller_errors() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));
        assert_eq!(status.render_body(), "provider error");

        status.observe_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn status_line_uses_compact_human_readable_provider_text() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        assert_eq!(status.text, "◐ working");
        assert!(status.provider_active());

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        assert_eq!(status.text, "reply ready");
        assert!(!status.provider_active());
    }

    #[test]
    fn status_line_cycles_terminal_safe_thinking_pulse() {
        let mut status = StatusLine::ready();

        status.start_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◓ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◑ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◒ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "reply ready");
    }
}
