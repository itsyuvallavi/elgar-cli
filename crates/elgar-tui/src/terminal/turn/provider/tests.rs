//! Tests for interactive provider turn rendering helpers.

use elgar_core::{event::ProviderStreamChunkReceived, provider::ProviderStreamChunk};

use super::{include_metrics_after_preserved_preview, should_skip_idle_repaint};
use crate::terminal::ui::prompt::LiveProviderOutput;

#[test]
fn idle_repaint_continues_before_answer_preview_exists() {
    let live_output = LiveProviderOutput::default();

    assert!(!should_skip_idle_repaint(&live_output));
}

#[test]
fn idle_repaint_stops_after_answer_preview_exists() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Text("Visible answer.".to_string()),
    ));

    assert!(should_skip_idle_repaint(&live_output));
}

#[test]
fn preserved_preview_keeps_final_metrics_line() {
    assert!(include_metrics_after_preserved_preview());
}
