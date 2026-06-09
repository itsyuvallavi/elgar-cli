//! Read-only local log diagnostics.
//!
//! This file formats existing `.elgar/log/system` JSONL data for humans. It
//! does not create logs or decide runtime behavior.

use std::{
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

pub const LOGS_COMMAND: &str = "logs";
pub const LOGS_LATEST_COMMAND: &str = "latest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsDiagnosticError {
    UnsupportedCommand,
    LogDirectoryMissing(PathBuf),
    NoSystemLogs(PathBuf),
    NoTurnPerfSummary(PathBuf),
    ReadFailed(String),
}

impl fmt::Display for LogsDiagnosticError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedCommand => write!(formatter, "usage: elgar logs latest"),
            Self::LogDirectoryMissing(path) => {
                write!(
                    formatter,
                    "system log directory does not exist: {}",
                    path.display()
                )
            }
            Self::NoSystemLogs(path) => {
                write!(
                    formatter,
                    "no system log files found under {}",
                    path.display()
                )
            }
            Self::NoTurnPerfSummary(path) => {
                write!(
                    formatter,
                    "no turn_perf_summary found under {}",
                    path.display()
                )
            }
            Self::ReadFailed(message) => write!(formatter, "{message}"),
        }
    }
}

impl Error for LogsDiagnosticError {}

pub fn is_logs_latest_command(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == LOGS_COMMAND)
}

pub fn render_logs_latest_from_args(
    args: &[String],
    project_root: &Path,
) -> Result<String, LogsDiagnosticError> {
    if !matches!(
        args,
        [command, subcommand] if command == LOGS_COMMAND && subcommand == LOGS_LATEST_COMMAND
    ) {
        return Err(LogsDiagnosticError::UnsupportedCommand);
    }

    render_latest_turn_summary(project_root)
}

pub fn render_latest_turn_summary(project_root: &Path) -> Result<String, LogsDiagnosticError> {
    let log_dir = elgar_core::log_directory(project_root);
    let entries = system_log_files_newest_first(&log_dir)?;
    for path in entries {
        if let Ok(summary) = latest_harness_summary(&path) {
            return Ok(render_harness_summary(&summary, &path));
        }
        if let Ok(summary) = latest_turn_perf_summary(&path) {
            return Ok(render_turn_perf_summary(&summary, &path));
        }
    }

    Err(LogsDiagnosticError::NoTurnPerfSummary(log_dir))
}

fn system_log_files_newest_first(log_dir: &Path) -> Result<Vec<PathBuf>, LogsDiagnosticError> {
    if !log_dir.exists() {
        return Err(LogsDiagnosticError::LogDirectoryMissing(
            log_dir.to_path_buf(),
        ));
    }

    let mut entries = fs::read_dir(log_dir)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("jsonl"))
                .then_some(path)
        })
        .filter_map(|path| {
            let modified = fs::metadata(&path).ok()?.modified().ok()?;
            Some((modified, path))
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|(modified, _path)| *modified);
    entries.reverse();
    let paths = entries
        .into_iter()
        .map(|(_modified, path)| path)
        .collect::<Vec<_>>();

    if paths.is_empty() {
        Err(LogsDiagnosticError::NoSystemLogs(log_dir.to_path_buf()))
    } else {
        Ok(paths)
    }
}

fn latest_turn_perf_summary(path: &Path) -> Result<Value, LogsDiagnosticError> {
    let contents = fs::read_to_string(path)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?;
    contents
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("summary").and_then(Value::as_str) == Some("turn_perf_summary"))
        .last()
        .ok_or_else(|| LogsDiagnosticError::NoTurnPerfSummary(path.to_path_buf()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HarnessDiagnosticSummary {
    session: String,
    duration_ms: Option<u64>,
    rounds: u64,
    stopped_reason: String,
    provider_calls: u64,
    backends: Vec<String>,
    tools: Vec<String>,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    repair_attempts: u64,
    synthesis_calls: u64,
    error: bool,
}

fn latest_harness_summary(path: &Path) -> Result<HarnessDiagnosticSummary, LogsDiagnosticError> {
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
    let turn_events = &events[started_index..=finished_index];
    let finished = &events[finished_index];
    let finished_metadata = finished.get("metadata").unwrap_or(&Value::Null);

    let mut backends = Vec::new();
    let mut tools = Vec::new();
    let mut prompt_tokens = 0;
    let mut completion_tokens = 0;
    let mut total_tokens = 0;
    let mut provider_calls = 0;
    let mut repair_attempts = 0;
    let mut synthesis_calls = 0;
    let mut error = false;

    for event in turn_events {
        let summary = event.get("summary").and_then(Value::as_str).unwrap_or("");
        let metadata = event.get("metadata").unwrap_or(&Value::Null);
        match summary {
            "harness_loop_provider_call_finished" | "harness_loop_synthesis_finished" => {
                provider_calls += 1;
                collect_unique_text(&mut backends, metadata, "backend");
                prompt_tokens += metadata
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                completion_tokens += metadata
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                total_tokens += metadata
                    .get("total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if summary == "harness_loop_synthesis_finished" {
                    synthesis_calls += 1;
                }
            }
            "harness_loop_provider_call_failed" => {
                provider_calls += 1;
                error = true;
            }
            "harness_loop_model_choice" => {
                collect_unique_text(&mut tools, metadata, "tool");
                if let Some(values) = metadata.get("tools").and_then(Value::as_array) {
                    for value in values {
                        if let Some(tool) = value.as_str() {
                            push_unique(&mut tools, tool.to_string());
                        }
                    }
                }
            }
            "harness_loop_evidence_collected" => {
                if let Some(label) = metadata.get("evidence_label").and_then(Value::as_str) {
                    if let Some((tool, _rest)) = label.split_once(':') {
                        push_unique(&mut tools, tool.to_string());
                    }
                }
            }
            "harness_loop_repair_finished" => {
                repair_attempts += 1;
            }
            _ => {}
        }
    }

    let stopped_reason = metadata_text(finished_metadata, "stopped_reason");
    if stopped_reason.contains("invalid") || stopped_reason.contains("failed") {
        error = true;
    }

    Ok(HarnessDiagnosticSummary {
        session: summary_text(finished, "session_id"),
        duration_ms: finished.get("duration_ms").and_then(Value::as_u64),
        rounds: metadata_count(finished_metadata, "rounds"),
        stopped_reason,
        provider_calls,
        backends,
        tools,
        prompt_tokens,
        completion_tokens,
        total_tokens,
        repair_attempts,
        synthesis_calls,
        error,
    })
}

fn render_harness_summary(summary: &HarnessDiagnosticSummary, path: &Path) -> String {
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

    [
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
        format!("provider calls: {}", summary.provider_calls),
        format!("tools: {tools}"),
        format!("repairs: {}", summary.repair_attempts),
        format!("synthesis: {}", summary.synthesis_calls),
        format!("error: {}", summary.error),
    ]
    .join("\n")
}

fn render_turn_perf_summary(summary: &Value, path: &Path) -> String {
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
