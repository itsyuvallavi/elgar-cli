//! Error types for local log diagnostics.

use std::{error::Error, fmt, path::PathBuf};

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
