use elgar_core::event::{AssistantMessageSource, Event, VerifiedActionResult};

use crate::markdown::render_assistant_markdown;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
    scrollback: ConversationScrollback,
}

impl ConversationPane {
    pub fn push_event(&mut self, event: &Event) {
        self.lines.push(render_tui_event(event));
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.scrollback.scroll_up(lines);
    }

    pub(crate) fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    pub(crate) fn follow_latest(&mut self) {
        self.scrollback.follow_latest();
    }

    pub(crate) fn scroll_offset(&self, viewport_height: u16) -> u16 {
        self.scrollback
            .offset_for(self.render_line_count(), usize::from(viewport_height))
    }

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
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ConversationScrollback {
    lines_from_bottom: usize,
}

impl ConversationScrollback {
    fn scroll_up(&mut self, lines: usize) {
        self.lines_from_bottom = self.lines_from_bottom.saturating_add(lines);
    }

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
            None => {
                "select visible text natively | PgUp/PgDn scroll | /copy conversation".to_string()
            }
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
}

impl StatusLine {
    pub fn ready() -> Self {
        Self {
            text: "ready".to_string(),
        }
    }

    pub fn observe_event(&mut self, event: &Event) {
        self.text = match event {
            Event::UserMessage(_) => "sent".to_string(),
            Event::AssistantMessage(_) => "reply ready".to_string(),
            Event::ProviderStarted(started) => {
                format!("provider working: {}", started.provider)
            }
            Event::ProviderFinished(finished) => {
                format!("provider response ready: {}", finished.provider)
            }
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
            Event::Error(error) => render_error_status(&error.message),
        };
    }

    pub(crate) fn render_body(&self) -> String {
        self.text.clone()
    }
}

fn render_tui_event(event: &Event) -> String {
    match event {
        Event::UserMessage(message) => format!("You: {}", message.content),
        Event::AssistantMessage(message) => {
            let speaker = match message.source {
                AssistantMessageSource::Controller => "Elgar",
                AssistantMessageSource::Provider => "Assistant suggestion",
            };
            render_assistant_output(speaker, &message.content)
        }
        Event::ProviderStarted(started) => {
            format!(
                "Provider progress: working with {} (request {}).",
                started.provider, started.request_id
            )
        }
        Event::ProviderFinished(finished) => {
            format!(
                "Provider progress: response ready from {} (request {}). Provider text is suggestion only.",
                finished.provider, finished.request_id
            )
        }
        Event::ActionProposed(action) => {
            format!(
                "Review needed: {} {:?} {}",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionApproved(action) => {
            format!(
                "Approved: {} {:?} {}",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionRejected(action) => {
            format!(
                "Rejected: {} {:?} {}. No file was changed.",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionApplied(applied) => {
            format!(
                "Applied and verified: {} {:?} {}",
                applied.action_id,
                applied.action_kind,
                render_verified_result(&applied.result)
            )
        }
        Event::ActionFailed(failed) => {
            format!(
                "Action failed: {} {:?} {}",
                failed.action_id, failed.action_kind, failed.reason
            )
        }
        Event::Error(error) => render_error_line(&error.message),
    }
}

fn render_assistant_output(speaker: &str, content: &str) -> String {
    let rendered = render_assistant_markdown(content);
    if rendered.contains('\n') {
        format!("{speaker}:\n{rendered}")
    } else {
        format!("{speaker}: {rendered}")
    }
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

fn render_error_status(message: &str) -> String {
    if parse_provider_error(message).is_some() {
        "provider error".to_string()
    } else {
        "error".to_string()
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
        assert!(rendered.contains("You: hello"));
        assert!(rendered.contains("Elgar: hi"));
        assert!(
            rendered.contains("Provider progress: working with stub-provider (request request-1).")
        );
        assert!(rendered.contains(
            "Provider progress: response ready from stub-provider (request request-1). Provider text is suggestion only."
        ));
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
        assert_eq!(
            CopyArea::default().render_hint(),
            "select visible text natively | PgUp/PgDn scroll | /copy conversation"
        );
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
        assert!(rendered.contains("Assistant suggestion:\nPlan:\n- read files\n- render output"));
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
        assert!(rendered.contains("Assistant suggestion:\n  File"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("changed"));
        assert!(!rendered.contains("| --- |"));
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
        assert_eq!(status.text, "provider working: stub-provider");

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        assert_eq!(status.text, "provider response ready: stub-provider");
    }
}
