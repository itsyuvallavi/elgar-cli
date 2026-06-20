//! Tests for compact live log rendering.

use super::render_follow_line;

#[test]
fn renders_provider_started_line() {
    let line = r#"{"timestamp_unix_ms":10,"summary":"harness_loop_provider_call_started","metadata":{"request_id":"request-1"}}"#;

    assert_eq!(
        render_follow_line(line).as_deref(),
        Some("10 request request-1 streaming")
    );
}

#[test]
fn renders_first_stream_chunk_line_only() {
    let first = r#"{"timestamp_unix_ms":20,"summary":"harness_loop_provider_stream_chunk","metadata":{"request_id":"request-1","sequence":1,"chunk_kind":"reasoning"}}"#;
    let later = r#"{"timestamp_unix_ms":21,"summary":"harness_loop_provider_stream_chunk","metadata":{"request_id":"request-1","sequence":2,"chunk_kind":"reasoning"}}"#;

    assert_eq!(
        render_follow_line(first).as_deref(),
        Some("20 request request-1 reasoning streaming")
    );
    assert_eq!(render_follow_line(later), None);
}

#[test]
fn renders_provider_finished_with_close_gap() {
    let line = r#"{"timestamp_unix_ms":30,"summary":"harness_loop_provider_call_finished","metadata":{"request_id":"request-1","total_stream_ms":26637,"stream_done_ms":20500,"last_chunk_to_done_ms":2,"done_to_finish_ms":1,"last_chunk_to_finish_ms":3,"total_tokens":1200}}"#;

    assert_eq!(
        render_follow_line(line).as_deref(),
        Some("30 request request-1 provider closed · total_stream_ms=26.6s · stream_done_ms=20.5s · last_chunk_to_done_ms=2ms · done_to_finish_ms=1ms · last_chunk_to_finish_ms=3ms · tokens=1200")
    );
}

#[test]
fn renders_memory_context_line() {
    let line = r#"{"timestamp_unix_ms":40,"summary":"harness_turn_prompt_context_built","metadata":{"system_prompt_chars":1200,"history_prompt_chars":400,"memory_prompt_chars":176,"mcp_catalog_chars":80,"total_initial_prompt_chars":1900,"history_token_budget":2048,"history_budget_hit":false,"assistant_replay_chars":220,"memory_selection_strategy":"recent_by_kind","indexed_fact_count":6,"rendered_fact_count":3,"omitted_fact_count":3,"rendered_memory_chars":176,"memory_budget_hit":false,"history_turns":2,"rendered_by_kind":{"read":1,"listed":1,"find":0,"grep":0,"executed":1}}}"#;

    assert_eq!(
        render_follow_line(line).as_deref(),
        Some("40 prompt chars system=1200 history=400 memory=176 mcp=80 total=1900 · history_budget=100/2048 hit=false assistant_replay=220\n40 memory strategy=recent_by_kind indexed=6 rendered=3 omitted=3 chars=176 budget_hit=false history=2 kinds=read:1 listed:1 find:0 grep:0 executed:1")
    );
}

#[test]
fn renders_session_context_status_line() {
    let line = r#"{"timestamp_unix_ms":50,"summary":"harness_session_context_status","metadata":{"turn_input_tokens":1200,"turn_output_tokens":43,"turn_total_tokens":1243,"session_input_tokens":4000,"session_output_tokens":600,"session_total_tokens":4600,"context_window_tokens":128000,"context_used_percent":3,"context_source":"provider","permission_mode":"review_all"}}"#;

    assert_eq!(
        render_follow_line(line).as_deref(),
        Some("50 tokens turn ↑1.2k ↓43 = 1.2k · session ↑4k ↓600 = 4.6k/128k (3%) · source provider · mode review_all")
    );
}

#[test]
fn renders_mcp_status_lines() {
    let active = r#"{"timestamp_unix_ms":60,"summary":"harness_mcp_status","metadata":{"mcp_active":true,"server_ids":["project-index","context7"],"source_path":"elgar-mcp.json"}}"#;
    let inactive = r#"{"timestamp_unix_ms":61,"summary":"harness_mcp_status","metadata":{"mcp_active":false}}"#;

    assert_eq!(
        render_follow_line(active).as_deref(),
        Some("60 mcp active servers=project-index,context7 source=elgar-mcp.json")
    );
    assert_eq!(
        render_follow_line(inactive).as_deref(),
        Some("61 mcp inactive")
    );
}
