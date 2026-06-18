//! Tests for interactive provider turn rendering helpers.

use elgar_core::{event::ProviderStreamChunkReceived, provider::ProviderStreamChunk};

use std::time::{Duration, Instant};

use super::{
    include_metrics_after_preserved_preview, should_render_idle_frame, should_render_stream_chunk,
};
use crate::terminal::ui::prompt::LiveProviderOutput;

#[test]
fn idle_repaint_continues_before_any_preview_exists() {
    let live_output = LiveProviderOutput::default();

    assert!(should_render_idle_frame(&live_output, false));
}

#[test]
fn idle_repaint_stops_after_unchanged_answer_preview_exists() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Text("Visible answer.".to_string()),
    ));

    assert!(!should_render_idle_frame(&live_output, false));
}

#[test]
fn idle_repaint_stops_after_unchanged_reasoning_preview_exists() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Reasoning("Need answer.".to_string()),
    ));

    assert!(!should_render_idle_frame(&live_output, false));
}

#[test]
fn idle_repaint_renders_dirty_live_preview_once() {
    let mut live_output = LiveProviderOutput::default();
    live_output.push_stream_chunk(&ProviderStreamChunkReceived::new(
        "provider",
        "request-1",
        1,
        ProviderStreamChunk::Reasoning("Need answer.".to_string()),
    ));

    assert!(should_render_idle_frame(&live_output, true));
}

#[test]
fn preserved_preview_keeps_final_metrics_line() {
    assert!(include_metrics_after_preserved_preview());
}

#[test]
fn stream_chunk_repaint_is_throttled() {
    assert!(!should_render_stream_chunk(Instant::now()));
    assert!(should_render_stream_chunk(
        Instant::now() - Duration::from_secs(1)
    ));
}
