//! Conversation scrollback and loading pulse state.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ConversationScrollback {
    lines_from_bottom: usize,
}

impl ConversationScrollback {
    pub(super) fn follow_latest(&mut self) {
        self.lines_from_bottom = 0;
    }

    pub(super) fn offset_for(&self, content_lines: usize, viewport_lines: usize) -> u16 {
        let max_offset = content_lines.saturating_sub(viewport_lines.max(1));
        max_offset
            .saturating_sub(self.lines_from_bottom.min(max_offset))
            .min(usize::from(u16::MAX)) as u16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::panes) struct ThinkingPulse {
    index: usize,
}

impl ThinkingPulse {
    const LABELS: [&'static str; 4] = ["working", "working.", "working..", "working..."];

    pub(in crate::panes) fn label(&self) -> &'static str {
        Self::LABELS[self.index]
    }

    pub(in crate::panes) fn reset(&mut self) {
        self.index = 0;
    }
}
