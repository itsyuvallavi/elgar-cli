//! Logical layout regions used by the TUI.
//!
//! These names let panes and renderers talk about screen areas without owning
//! the actual terminal drawing code.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRegion {
    Conversation,
    Input,
    Status,
    PendingAction,
}

impl LayoutRegion {
    pub fn title(self) -> &'static str {
        match self {
            Self::Conversation => "Conversation",
            Self::Input => "Input",
            Self::Status => "Status",
            Self::PendingAction => "Pending Action",
        }
    }
}

pub(crate) fn render_section(title: &str, body: &str) -> String {
    format!("{title}\n{body}\n")
}

#[cfg(test)]
mod tests;
