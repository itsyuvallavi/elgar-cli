//! Compact rendering for live system-log follow output.
//!
//! This module only formats existing JSONL events. It does not read files,
//! create logs, or decide runtime behavior.

use serde_json::Value;

pub(super) fn render_follow_line(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let summary = value.get("summary").and_then(Value::as_str)?;
    let metadata = value.get("metadata").unwrap_or(&Value::Null);

    match summary {
        "harness_loop_provider_call_started" => Some(format!(
            "{} request {} streaming",
            timestamp(&value),
            metadata_text(metadata, "request_id")
        )),
        "harness_loop_provider_stream_chunk" => render_stream_chunk(&value, metadata),
        "harness_synthesis_provider_stream_chunk" => render_stream_chunk(&value, metadata),
        "harness_loop_provider_call_finished" => Some(render_provider_finished(&value, metadata)),
        "harness_loop_synthesis_finished" => Some(render_provider_finished(&value, metadata)),
        "harness_turn_prompt_context_built" => Some(render_memory_context(&value, metadata)),
        "harness_session_context_status" => Some(render_session_context_status(&value, metadata)),
        "harness_mcp_status" => Some(render_mcp_status(&value, metadata)),
        "harness_approval_requested" => Some(render_approval_requested(&value, metadata)),
        "harness_approval_decision" => Some(render_approval_decision(&value, metadata)),
        "provider_worker_completion_received" => Some(format!(
            "{} request {} worker received",
            timestamp(&value),
            metadata_text(metadata, "latest_provider_request_id")
        )),
        "ui_render_finished" | "scripted_tui_render_finished" => Some(format!(
            "{} request {} rendered{}",
            timestamp(&value),
            metadata_text(metadata, "latest_provider_request_id"),
            duration_suffix(metadata, "completion_to_render_ms")
        )),
        "harness_loop_provider_call_failed" => Some(format!(
            "{} request {} failed: {}",
            timestamp(&value),
            metadata_text(metadata, "request_id"),
            metadata_text(metadata, "error_kind")
        )),
        "harness_loop_finished" => Some(format!(
            "{} turn stopped: {}",
            timestamp(&value),
            metadata_text(metadata, "stopped_reason")
        )),
        _ => None,
    }
}

fn render_stream_chunk(value: &Value, metadata: &Value) -> Option<String> {
    let sequence = metadata.get("sequence").and_then(Value::as_u64)?;
    (sequence == 1).then(|| {
        format!(
            "{} request {} {} streaming",
            timestamp(value),
            metadata_text(metadata, "request_id"),
            metadata_text(metadata, "chunk_kind")
        )
    })
}

fn render_provider_finished(value: &Value, metadata: &Value) -> String {
    format!(
        "{} request {} provider closed{}{}{}{}{}{}",
        timestamp(value),
        metadata_text(metadata, "request_id"),
        duration_suffix(metadata, "total_stream_ms"),
        duration_suffix(metadata, "stream_done_ms"),
        duration_suffix(metadata, "last_chunk_to_done_ms"),
        duration_suffix(metadata, "done_to_finish_ms"),
        duration_suffix(metadata, "last_chunk_to_finish_ms"),
        token_suffix(metadata)
    )
}

fn render_memory_context(value: &Value, metadata: &Value) -> String {
    format!(
        "{} memory strategy={} indexed={} rendered={} omitted={} chars={} budget_hit={} history={} kinds={}",
        timestamp(value),
        metadata_text(metadata, "memory_selection_strategy"),
        metadata_count(metadata, "indexed_fact_count"),
        metadata_count(metadata, "rendered_fact_count"),
        metadata_count(metadata, "omitted_fact_count"),
        metadata_count(metadata, "rendered_memory_chars"),
        metadata_bool(metadata, "memory_budget_hit"),
        metadata_count(metadata, "history_turns"),
        rendered_kind_summary(metadata)
    )
}

fn rendered_kind_summary(metadata: &Value) -> String {
    let rendered = metadata.get("rendered_by_kind").unwrap_or(&Value::Null);
    format!(
        "read:{} listed:{} find:{} grep:{} executed:{}",
        metadata_count(rendered, "read"),
        metadata_count(rendered, "listed"),
        metadata_count(rendered, "find"),
        metadata_count(rendered, "grep"),
        metadata_count(rendered, "executed")
    )
}

fn render_session_context_status(value: &Value, metadata: &Value) -> String {
    format!(
        "{} tokens turn {} · session {} · mode {}{}",
        timestamp(value),
        turn_token_summary(metadata),
        session_context_summary(metadata),
        metadata_text(metadata, "permission_mode"),
        pending_approval_suffix(metadata)
    )
}

fn render_mcp_status(value: &Value, metadata: &Value) -> String {
    let active = metadata.get("mcp_active").and_then(Value::as_bool) == Some(true);
    if active {
        format!(
            "{} mcp active servers={} source={}",
            timestamp(value),
            text_list(metadata, "server_ids"),
            metadata_text(metadata, "source_path")
        )
    } else {
        format!("{} mcp inactive", timestamp(value))
    }
}

fn render_approval_requested(value: &Value, metadata: &Value) -> String {
    format!(
        "{} approval pending {} {} target={} scope={}",
        timestamp(value),
        metadata_text(metadata, "tool"),
        metadata_text(metadata, "approval_id"),
        metadata_text(metadata, "target"),
        metadata_text(metadata, "target_scope")
    )
}

fn render_approval_decision(value: &Value, metadata: &Value) -> String {
    format!(
        "{} approval {} {}",
        timestamp(value),
        metadata_text(metadata, "status"),
        metadata_text(metadata, "approval_id")
    )
}

fn turn_token_summary(metadata: &Value) -> String {
    format!(
        "{} {} = {}",
        compact_optional_count(metadata, "turn_input_tokens", "↑"),
        compact_optional_count(metadata, "turn_output_tokens", "↓"),
        compact_optional_count(metadata, "turn_total_tokens", "")
    )
}

fn session_context_summary(metadata: &Value) -> String {
    let current = compact_optional_count(metadata, "session_total_tokens", "");
    let window = compact_optional_count(metadata, "context_window_tokens", "");
    let percent = metadata
        .get("context_used_percent")
        .and_then(Value::as_u64)
        .map(|value| format!(" ({value}%)"))
        .unwrap_or_default();
    format!("{current}/{window}{percent}")
}

fn pending_approval_suffix(metadata: &Value) -> String {
    let Some(id) = metadata
        .get("pending_approval_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return String::new();
    };
    format!(
        " · pending {} {}",
        metadata_text(metadata, "pending_approval_tool"),
        id
    )
}

fn timestamp(value: &Value) -> String {
    value
        .get("timestamp_unix_ms")
        .and_then(Value::as_u64)
        .map(|millis| millis.to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn metadata_text(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string()
}

fn metadata_count(metadata: &Value, key: &str) -> u64 {
    metadata.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn metadata_bool(metadata: &Value, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn text_list(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "?".to_string())
}

fn duration_suffix(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_u64)
        .map(|millis| format!(" · {key}={}", format_duration(millis)))
        .unwrap_or_default()
}

fn token_suffix(metadata: &Value) -> String {
    metadata
        .get("total_tokens")
        .and_then(Value::as_u64)
        .map(|tokens| format!(" · tokens={tokens}"))
        .unwrap_or_default()
}

fn compact_optional_count(metadata: &Value, key: &str, prefix: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| format!("{prefix}{}", compact_count(value)))
        .unwrap_or_else(|| format!("{prefix}?"))
}

fn format_duration(millis: u64) -> String {
    if millis < 1_000 {
        format!("{millis}ms")
    } else {
        format!("{:.1}s", millis as f64 / 1_000.0)
    }
}

fn compact_count(value: u64) -> String {
    if value >= 1_000 {
        let thousands = value as f64 / 1_000.0;
        if value.is_multiple_of(1_000) {
            format!("{thousands:.0}k")
        } else {
            format!("{thousands:.1}k")
        }
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests;
