//! Tests for logical TUI layout regions.

use super::super::{render_section, LayoutRegion};

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
