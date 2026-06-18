//! Harness-loop diagnostic summary extraction from JSONL events.

use std::{fs, path::Path};

use serde_json::Value;

use super::LogsDiagnosticError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HarnessDiagnosticSummary {
    pub(super) session: String,
    pub(super) duration_ms: Option<u64>,
    pub(super) rounds: u64,
    pub(super) stopped_reason: String,
    pub(super) provider_calls: u64,
    pub(super) backends: Vec<String>,
    pub(super) tools: Vec<String>,
    pub(super) prompt_tokens: u64,
    pub(super) completion_tokens: u64,
    pub(super) total_tokens: u64,
    pub(super) repair_attempts: u64,
    pub(super) permission_prompts: u64,
    pub(super) permission_approved: u64,
    pub(super) permission_denied: u64,
    pub(super) synthesis_calls: u64,
    pub(super) error: bool,
    pub(super) memory: Option<MemoryDiagnosticSummary>,
    pub(super) context: Option<ContextDiagnosticSummary>,
    pub(super) mcp: Option<McpDiagnosticSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MemoryDiagnosticSummary {
    pub(super) indexed_facts: u64,
    pub(super) rendered_facts: u64,
    pub(super) omitted_facts: u64,
    pub(super) rendered_chars: u64,
    pub(super) budget_hit: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ContextDiagnosticSummary {
    pub(super) total_tokens: Option<u64>,
    pub(super) window_tokens: Option<u64>,
    pub(super) used_percent: Option<u64>,
    pub(super) permission_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpDiagnosticSummary {
    pub(super) active: bool,
    pub(super) server_ids: Vec<String>,
    pub(super) source_path: Option<String>,
    pub(super) error: Option<String>,
}

pub(super) fn latest_harness_summary(
    path: &Path,
) -> Result<HarnessDiagnosticSummary, LogsDiagnosticError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?;
    let events = contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect::<Vec<_>>();
    let Some(finished_index) = events.iter().rposition(|value| {
        value.get("summary").and_then(Value::as_str) == Some("harness_loop_finished")
    }) else {
        return Err(LogsDiagnosticError::NoTurnPerfSummary(path.to_path_buf()));
    };
    let started_index = events[..=finished_index]
        .iter()
        .rposition(|value| {
            value.get("summary").and_then(Value::as_str) == Some("harness_turn_started")
        })
        .unwrap_or(0);
    let next_started_index = events[finished_index + 1..]
        .iter()
        .position(|value| {
            value.get("summary").and_then(Value::as_str) == Some("harness_turn_started")
        })
        .map(|index| finished_index + 1 + index)
        .unwrap_or(events.len());
    let turn_events = &events[started_index..next_started_index];
    let finished = &events[finished_index];
    let finished_metadata = finished.get("metadata").unwrap_or(&Value::Null);

    let mut summary = HarnessDiagnosticSummary {
        session: summary_text(finished, "session_id"),
        duration_ms: finished.get("duration_ms").and_then(Value::as_u64),
        rounds: metadata_count(finished_metadata, "rounds"),
        stopped_reason: metadata_text(finished_metadata, "stopped_reason"),
        provider_calls: 0,
        backends: Vec::new(),
        tools: Vec::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        repair_attempts: 0,
        permission_prompts: 0,
        permission_approved: 0,
        permission_denied: 0,
        synthesis_calls: 0,
        error: false,
        memory: None,
        context: None,
        mcp: None,
    };

    for event in turn_events {
        observe_event(&mut summary, event);
    }

    if summary.stopped_reason.contains("invalid") || summary.stopped_reason.contains("failed") {
        summary.error = true;
    }

    Ok(summary)
}

fn observe_event(summary: &mut HarnessDiagnosticSummary, event: &Value) {
    let event_summary = event.get("summary").and_then(Value::as_str).unwrap_or("");
    let metadata = event.get("metadata").unwrap_or(&Value::Null);
    match event_summary {
        "harness_loop_provider_call_finished" | "harness_loop_synthesis_finished" => {
            summary.provider_calls += 1;
            collect_unique_text(&mut summary.backends, metadata, "backend");
            summary.prompt_tokens += metadata_count(metadata, "prompt_tokens");
            summary.completion_tokens += metadata_count(metadata, "completion_tokens");
            summary.total_tokens += metadata_count(metadata, "total_tokens");
            if event_summary == "harness_loop_synthesis_finished" {
                summary.synthesis_calls += 1;
            }
        }
        "harness_loop_provider_call_failed" => {
            summary.provider_calls += 1;
            summary.error = true;
        }
        "harness_loop_model_choice" => {
            collect_unique_text(&mut summary.tools, metadata, "tool");
            if let Some(values) = metadata.get("tools").and_then(Value::as_array) {
                for value in values {
                    if let Some(tool) = value.as_str() {
                        push_unique(&mut summary.tools, tool.to_string());
                    }
                }
            }
        }
        "harness_loop_evidence_collected" => {
            if let Some(label) = metadata.get("evidence_label").and_then(Value::as_str) {
                if let Some((tool, _rest)) = label.split_once(':') {
                    push_unique(&mut summary.tools, tool.to_string());
                }
            }
        }
        "harness_loop_repair_finished" => summary.repair_attempts += 1,
        "harness_turn_prompt_context_built" => {
            summary.memory = Some(MemoryDiagnosticSummary {
                indexed_facts: metadata_count(metadata, "indexed_fact_count"),
                rendered_facts: metadata_count(metadata, "rendered_fact_count"),
                omitted_facts: metadata_count(metadata, "omitted_fact_count"),
                rendered_chars: metadata_count(metadata, "rendered_memory_chars"),
                budget_hit: metadata_bool(metadata, "memory_budget_hit"),
            });
        }
        "harness_session_context_status" => {
            summary.context = Some(ContextDiagnosticSummary {
                total_tokens: metadata.get("session_total_tokens").and_then(Value::as_u64),
                window_tokens: metadata
                    .get("context_window_tokens")
                    .and_then(Value::as_u64),
                used_percent: metadata.get("context_used_percent").and_then(Value::as_u64),
                permission_mode: metadata_text(metadata, "permission_mode"),
            });
        }
        "harness_mcp_status" => {
            summary.mcp = Some(McpDiagnosticSummary {
                active: metadata_bool(metadata, "mcp_active"),
                server_ids: metadata_text_array(metadata, "server_ids"),
                source_path: metadata_optional_text(metadata, "source_path"),
                error: metadata_optional_text(metadata, "error"),
            });
        }
        "harness_permission_decision" => {
            collect_unique_text(&mut summary.tools, metadata, "tool");
            if metadata.get("decision").and_then(Value::as_str) == Some("deny") {
                summary.permission_denied += 1;
            }
        }
        "harness_approval_decision" => match metadata.get("status").and_then(Value::as_str) {
            Some("approved") => summary.permission_approved += 1,
            Some("denied") => summary.permission_denied += 1,
            _ => {}
        },
        "harness_approval_requested" => {
            collect_unique_text(&mut summary.tools, metadata, "tool");
            if metadata.get("status").and_then(Value::as_str) == Some("pending") {
                summary.permission_prompts += 1;
            }
        }
        "harness_bash_execution_started"
        | "harness_bash_execution_finished"
        | "harness_write_execution_started"
        | "harness_write_execution_finished"
        | "harness_edit_execution_started"
        | "harness_edit_execution_finished" => {
            collect_unique_text(&mut summary.tools, metadata, "tool")
        }
        _ => {}
    }
}

fn collect_unique_text(values: &mut Vec<String>, metadata: &Value, key: &str) {
    if let Some(value) = metadata.get(key).and_then(Value::as_str) {
        push_unique(values, value.to_string());
    }
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
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

fn metadata_optional_text(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn metadata_count(metadata: &Value, key: &str) -> u64 {
    metadata.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn metadata_bool(metadata: &Value, key: &str) -> bool {
    metadata.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn metadata_text_array(metadata: &Value, key: &str) -> Vec<String> {
    metadata
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}
