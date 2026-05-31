use elgar_core::event::Event;

use super::{conversation::ThinkingPulse, event_rendering::parse_provider_error};

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
