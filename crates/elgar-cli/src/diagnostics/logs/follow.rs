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

use super::{follow_render::render_follow_line, scan, LogsDiagnosticError};

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

#[cfg(test)]
mod tests {
    use std::{
        fs::{self, File},
        io::Write,
    };

    use super::start_offset_for_file;

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
