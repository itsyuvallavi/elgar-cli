//! Human-readable rendering for log diagnostic summaries.

use std::path::Path;

use serde_json::Value;

use super::summary::{
    ContextDiagnosticSummary, HarnessDiagnosticSummary, McpDiagnosticSummary,
    MemoryDiagnosticSummary,
};

pub(super) fn render_harness_summary(summary: &HarnessDiagnosticSummary, path: &Path) -> String {
    let backend = if summary.backends.is_empty() {
        "?".to_string()
    } else {
        summary.backends.join(", ")
    };
    let tools = if summary.tools.is_empty() {
        "none".to_string()
    } else {
        summary.tools.join(", ")
    };
    let duration = summary
        .duration_ms
        .map(format_duration)
        .unwrap_or_else(|| "?".to_string());

    let mut lines = vec![
        "Latest harness summary".to_string(),
        format!("file: {}", path.display()),
        format!("session: {}", summary.session),
        format!("backend: {backend}"),
        format!("duration: {duration}"),
        format!("rounds: {}", summary.rounds),
        format!("stop reason: {}", summary.stopped_reason),
        format!(
            "tokens: ↑{} ↓{} = {}",
            compact_count(summary.prompt_tokens),
            compact_count(summary.completion_tokens),
            compact_count(summary.total_tokens)
        ),
    ];
    lines.push(render_context_line(summary.context.as_ref()));
    lines.push(render_memory_line(summary.memory.as_ref()));
    lines.push(render_mcp_line(summary.mcp.as_ref()));
    lines.extend([
        format!("provider calls: {}", summary.provider_calls),
        format!("tools: {tools}"),
        format!(
            "permissions: prompts {} · approved {} · denied {}",
            summary.permission_prompts, summary.permission_approved, summary.permission_denied
        ),
        format!("repairs: {}", summary.repair_attempts),
        format!("synthesis: {}", summary.synthesis_calls),
        format!("error: {}", summary.error),
    ]);
    lines.join("\n")
}

fn render_context_line(context: Option<&ContextDiagnosticSummary>) -> String {
    let Some(context) = context else {
        return "context: ?".to_string();
    };
    let total = context
        .total_tokens
        .map(compact_count)
        .unwrap_or_else(|| "?".to_string());
    let window = context
        .window_tokens
        .map(compact_count)
        .unwrap_or_else(|| "?".to_string());
    let percent = context
        .used_percent
        .map(|value| format!(" ({value}%)"))
        .unwrap_or_default();

    format!(
        "context: {total}/{window}{percent} · mode {}",
        context.permission_mode
    )
}

fn render_memory_line(memory: Option<&MemoryDiagnosticSummary>) -> String {
    let Some(memory) = memory else {
        return "memory: ?".to_string();
    };
    format!(
        "memory: strategy {} · indexed {} · rendered {} · omitted {} · chars {} · budget_hit {}",
        memory.selection_strategy,
        memory.indexed_facts,
        memory.rendered_facts,
        memory.omitted_facts,
        memory.rendered_chars,
        memory.budget_hit
    )
}

fn render_mcp_line(mcp: Option<&McpDiagnosticSummary>) -> String {
    let Some(mcp) = mcp else {
        return "mcp: ?".to_string();
    };
    if !mcp.active {
        if let Some(error) = mcp.error.as_deref() {
            return format!("mcp: inactive · error {error}");
        }
        return "mcp: inactive".to_string();
    }
    let servers = if mcp.server_ids.is_empty() {
        "?".to_string()
    } else {
        mcp.server_ids.join(", ")
    };
    let source = mcp
        .source_path
        .as_deref()
        .map(|path| format!(" · source {path}"))
        .unwrap_or_default();
    format!("mcp: active · servers {servers}{source}")
}

pub(super) fn render_turn_perf_summary(summary: &Value, path: &Path) -> String {
    let metadata = summary.get("metadata").unwrap_or(&Value::Null);
    let session = summary_text(summary, "session_id");
    let backend = metadata_text(metadata, "backend");
    let duration = summary
        .get("duration_ms")
        .and_then(Value::as_u64)
        .map(format_duration)
        .unwrap_or_else(|| "?".to_string());
    let provider_duration = metadata
        .get("total_provider_duration_millis")
        .and_then(Value::as_u64)
        .map(format_duration)
        .unwrap_or_else(|| "?".to_string());
    let input = metadata_token(metadata, "prompt_tokens");
    let output = metadata_token(metadata, "completion_tokens");
    let total = metadata_token(metadata, "total_tokens");

    [
        "Latest turn summary".to_string(),
        format!("file: {}", path.display()),
        format!("session: {session}"),
        format!("backend: {backend}"),
        format!("duration: {duration}"),
        format!("provider duration: {provider_duration}"),
        format!("tokens: ↑{input} ↓{output} = {total}"),
        format!("loops: {}", metadata_count(metadata, "loop_round_count")),
        format!(
            "provider calls: {} (api {}, unknown {})",
            metadata_count(metadata, "provider_request_count"),
            metadata_count(metadata, "api_call_count"),
            metadata_count(metadata, "unknown_provider_call_count")
        ),
        format!(
            "tools: requests {} · executions {}",
            metadata_count(metadata, "tool_request_count"),
            metadata_count(metadata, "tool_execution_count")
        ),
        format!(
            "permissions: prompts {} · approved {} · denied {}",
            metadata_count(metadata, "permission_prompt_count"),
            metadata_count(metadata, "permission_approved_count"),
            metadata_count(metadata, "permission_denied_count")
        ),
        format!("synthesis: {}", metadata_count(metadata, "synthesis_count")),
        format!("stream: {}", metadata_bool(metadata, "stream")),
        format!("error: {}", metadata_bool(metadata, "error")),
    ]
    .join("\n")
}

fn summary_text(summary: &Value, key: &str) -> String {
    summary
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string()
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

fn metadata_token(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_u64)
        .map(compact_count)
        .unwrap_or_else(|| "?".to_string())
}

fn metadata_bool(metadata: &Value, key: &str) -> String {
    metadata
        .get(key)
        .and_then(Value::as_bool)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "?".to_string())
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
