//! Read-only local log diagnostics.
//!
//! This module formats existing `.elgar/log/system` JSONL data for humans. It
//! does not create logs or decide runtime behavior.

use std::{io::Write, path::Path};

mod follow;
mod follow_render;
mod render;
mod scan;
mod summary;
mod types;

pub use types::LogsDiagnosticError;

pub const LOGS_COMMAND: &str = "logs";
pub const LOGS_LATEST_COMMAND: &str = "latest";
pub const LOGS_FOLLOW_COMMAND: &str = "--follow";
pub const LOGS_FOLLOW_SHORT_COMMAND: &str = "-f";
pub const LOGS_FOLLOW_ALIAS_COMMAND: &str = "follow";

pub fn is_logs_command(args: &[String]) -> bool {
    args.first().is_some_and(|arg| arg == LOGS_COMMAND)
}

pub fn run_logs_from_args<W: Write>(
    args: &[String],
    project_root: &Path,
    writer: &mut W,
) -> Result<(), LogsDiagnosticError> {
    match args {
        [command, subcommand] if command == LOGS_COMMAND && subcommand == LOGS_LATEST_COMMAND => {
            writeln!(writer, "{}", render_latest_turn_summary(project_root)?)
                .map_err(|error| LogsDiagnosticError::WriteFailed(error.to_string()))
        }
        [command, subcommand]
            if command == LOGS_COMMAND
                && matches!(
                    subcommand.as_str(),
                    LOGS_FOLLOW_COMMAND | LOGS_FOLLOW_SHORT_COMMAND | LOGS_FOLLOW_ALIAS_COMMAND
                ) =>
        {
            follow::follow_system_logs(project_root, writer)
        }
        _ => Err(LogsDiagnosticError::UnsupportedCommand),
    }
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
