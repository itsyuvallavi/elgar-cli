//! Read-only local log diagnostics.
//!
//! This module formats existing `.elgar/log/system` JSONL data for humans. It
//! does not create logs or decide runtime behavior.

use std::path::Path;

mod render;
mod scan;
mod summary;
mod types;

pub use types::LogsDiagnosticError;

pub const LOGS_COMMAND: &str = "logs";
pub const LOGS_LATEST_COMMAND: &str = "latest";

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
    let entries = scan::system_log_files_newest_first(&log_dir)?;
    for path in entries {
        if let Ok(summary) = summary::latest_harness_summary(&path) {
            return Ok(render::render_harness_summary(&summary, &path));
        }
        if let Ok(summary) = scan::latest_turn_perf_summary(&path) {
            return Ok(render::render_turn_perf_summary(&summary, &path));
        }
    }

    Err(LogsDiagnosticError::NoTurnPerfSummary(log_dir))
}
