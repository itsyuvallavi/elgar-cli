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
mod tests {
    use super::{render_section, LayoutRegion};

    #[test]
    fn layout_region_titles_match_existing_rendering() {
        assert_eq!(LayoutRegion::Conversation.title(), "Conversation");
        assert_eq!(LayoutRegion::PendingAction.title(), "Pending Action");
        assert_eq!(LayoutRegion::Status.title(), "Status");
        assert_eq!(LayoutRegion::Input.title(), "Input");
    }

    #[test]
    fn section_rendering_keeps_existing_shape() {
        assert_eq!(render_section("Status", "ready"), "Status\nready\n");
    }
}
