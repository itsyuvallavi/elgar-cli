//! Live terminal follower for local system JSONL logs.
//!
//! This module is read-only. It tails `.elgar/log/system` and renders compact
//! request lifecycle lines for debugging provider/TUI latency.

use std::{
    fs::File,
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use serde_json::Value;

use super::{scan, LogsDiagnosticError};

const POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(super) fn follow_system_logs<W: Write>(
    project_root: &Path,
    writer: &mut W,
) -> Result<(), LogsDiagnosticError> {
    let log_dir = elgar_core::log_directory(project_root);
    let mut followed_path = None::<PathBuf>;
    let mut offset = 0_u64;

    writeln!(writer, "Following system logs under {}", log_dir.display())
        .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))?;
    writer
        .flush()
        .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))?;

    loop {
        if let Some(path) = newest_system_log(&log_dir)? {
            if followed_path.as_ref() != Some(&path) {
                let first_attach = followed_path.is_none();
                followed_path = Some(path.clone());
                offset = start_offset_for_file(&path, first_attach)?;
                writeln!(writer, "file {}", path.display())
                    .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))?;
            }

            offset = render_new_lines(&path, offset, writer)?;
            writer
                .flush()
                .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))?;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn newest_system_log(log_dir: &Path) -> Result<Option<PathBuf>, LogsDiagnosticError> {
    match scan::system_log_files_newest_first(log_dir) {
        Ok(paths) => Ok(paths.into_iter().next()),
        Err(LogsDiagnosticError::LogDirectoryMissing(_))
        | Err(LogsDiagnosticError::NoSystemLogs(_)) => Ok(None),
        Err(error) => Err(error),
    }
}

fn start_offset_for_file(path: &Path, first_attach: bool) -> Result<u64, LogsDiagnosticError> {
    if first_attach {
        return path
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()));
    }

    Ok(0)
}

fn render_new_lines<W: Write>(
    path: &Path,
    offset: u64,
    writer: &mut W,
) -> Result<u64, LogsDiagnosticError> {
    let mut file =
        File::open(path).map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?;
    let file_len = file
        .metadata()
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?
        .len();
    let safe_offset = offset.min(file_len);
    file.seek(SeekFrom::Start(safe_offset))
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?;

    let mut reader = BufReader::new(file);
    let mut current_offset = safe_offset;
    let mut line = String::new();
    while reader
        .read_line(&mut line)
        .map_err(|error| LogsDiagnosticError::ReadFailed(error.to_string()))?
        > 0
    {
        current_offset = current_offset.saturating_add(line.as_bytes().len() as u64);
        if let Some(rendered) = render_follow_line(line.trim_end()) {
            writeln!(writer, "{rendered}")
                .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))?;
        }
        line.clear();
    }

    Ok(current_offset)
}

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
        "harness_synthesis_finished" => Some(render_provider_finished(&value, metadata)),
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

fn format_duration(millis: u64) -> String {
    if millis < 1_000 {
        format!("{millis}ms")
    } else {
        format!("{:.1}s", millis as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use super::{render_follow_line, start_offset_for_file};

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
    fn first_attach_starts_at_end_of_existing_file() {
        let path = temp_log_path("first-attach");
        fs::write(&path, b"old\nlines\n").expect("write temp log");

        let offset = start_offset_for_file(&path, true).expect("offset");

        assert_eq!(offset, 10);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn later_file_switch_starts_at_beginning() {
        let path = temp_log_path("later-switch");
        let mut file = File::create(&path).expect("create temp log");
        file.write_all(b"new\n").expect("write temp log");

        let offset = start_offset_for_file(&path, false).expect("offset");

        assert_eq!(offset, 0);
        let _ = fs::remove_file(path);
    }

    fn temp_log_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "elgar-follow-{name}-{}-{}.jsonl",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }
}
