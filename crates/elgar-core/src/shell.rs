use std::{
    fmt,
    io::{self, Read},
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use crate::{
    action::{ShellActionVerification, ShellCommandAction},
    event::VerifiedActionResult,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellExecutor;

impl ShellExecutor {
    pub fn execute(
        action: &ShellCommandAction,
    ) -> Result<VerifiedActionResult, ShellExecutionError> {
        execute_shell_command(action).map(VerifiedActionResult::Shell)
    }
}

pub fn execute_shell_command(
    action: &ShellCommandAction,
) -> Result<ShellActionVerification, ShellExecutionError> {
    let started = Instant::now();
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&action.command)
        .current_dir(&action.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| ShellExecutionError::SpawnFailed {
            cwd: action.cwd.clone(),
            reason: source.to_string(),
        })?;

    let stdout = child
        .stdout
        .take()
        .ok_or(ShellExecutionError::MissingPipe { stream: "stdout" })?;
    let stderr = child
        .stderr
        .take()
        .ok_or(ShellExecutionError::MissingPipe { stream: "stderr" })?;
    let stdout_cap = action.output_caps.stdout_bytes;
    let stderr_cap = action.output_caps.stderr_bytes;
    let stdout_reader = thread::spawn(move || read_capped(stdout, stdout_cap));
    let stderr_reader = thread::spawn(move || read_capped(stderr, stderr_cap));

    let timeout = Duration::from_secs(action.timeout_seconds);
    let mut timed_out = false;
    let status = loop {
        if let Some(status) =
            child
                .try_wait()
                .map_err(|source| ShellExecutionError::WaitFailed {
                    reason: source.to_string(),
                })?
        {
            break status;
        }

        if started.elapsed() >= timeout {
            timed_out = true;
            child
                .kill()
                .map_err(|source| ShellExecutionError::KillFailed {
                    reason: source.to_string(),
                })?;
            break child
                .wait()
                .map_err(|source| ShellExecutionError::WaitFailed {
                    reason: source.to_string(),
                })?;
        }

        thread::sleep(Duration::from_millis(10));
    };

    let stdout = join_output_reader(stdout_reader, "stdout")?;
    let stderr = join_output_reader(stderr_reader, "stderr")?;

    Ok(ShellActionVerification {
        command: action.command.clone(),
        cwd: action.cwd.display().to_string(),
        stdout: stdout.text,
        stderr: stderr.text,
        stdout_truncated: stdout.truncated,
        stderr_truncated: stderr.truncated,
        exit_code: if timed_out { None } else { status.code() },
        elapsed_millis: millis_since(started),
        timed_out,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CappedOutput {
    text: String,
    truncated: bool,
}

fn read_capped(mut reader: impl Read, cap: usize) -> io::Result<CappedOutput> {
    let mut output = Vec::new();
    let mut buffer = [0; 8192];
    let mut total_bytes = 0usize;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }

        total_bytes = total_bytes.saturating_add(bytes_read);
        let remaining = cap.saturating_sub(output.len());
        let bytes_to_keep = remaining.min(bytes_read);
        output.extend_from_slice(&buffer[..bytes_to_keep]);
    }

    Ok(CappedOutput {
        text: capped_string(output, cap),
        truncated: total_bytes > cap,
    })
}

fn capped_string(output: Vec<u8>, cap: usize) -> String {
    let mut output = String::from_utf8_lossy(&output).into_owned();
    if output.len() <= cap {
        return output;
    }

    let mut truncate_at = cap;
    while truncate_at > 0 && !output.is_char_boundary(truncate_at) {
        truncate_at -= 1;
    }
    output.truncate(truncate_at);
    output
}

fn join_output_reader(
    reader: thread::JoinHandle<io::Result<CappedOutput>>,
    stream: &'static str,
) -> Result<CappedOutput, ShellExecutionError> {
    reader
        .join()
        .map_err(|_| ShellExecutionError::OutputReaderPanicked { stream })?
        .map_err(|source| ShellExecutionError::OutputReadFailed {
            stream,
            reason: source.to_string(),
        })
}

fn millis_since(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellExecutionError {
    SpawnFailed {
        cwd: PathBuf,
        reason: String,
    },
    MissingPipe {
        stream: &'static str,
    },
    OutputReadFailed {
        stream: &'static str,
        reason: String,
    },
    OutputReaderPanicked {
        stream: &'static str,
    },
    WaitFailed {
        reason: String,
    },
    KillFailed {
        reason: String,
    },
}

impl fmt::Display for ShellExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShellExecutionError::SpawnFailed { cwd, reason } => {
                write!(
                    formatter,
                    "failed to spawn shell command in {}: {reason}",
                    cwd.display()
                )
            }
            ShellExecutionError::MissingPipe { stream } => {
                write!(formatter, "failed to capture shell command {stream}")
            }
            ShellExecutionError::OutputReadFailed { stream, reason } => {
                write!(formatter, "failed to read shell command {stream}: {reason}")
            }
            ShellExecutionError::OutputReaderPanicked { stream } => {
                write!(formatter, "shell command {stream} reader panicked")
            }
            ShellExecutionError::WaitFailed { reason } => {
                write!(formatter, "failed to wait for shell command: {reason}")
            }
            ShellExecutionError::KillFailed { reason } => {
                write!(
                    formatter,
                    "failed to stop timed out shell command: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for ShellExecutionError {}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::action::{ShellCommandAction, ShellCommandOutputCaps};

    use super::{execute_shell_command, ShellExecutor};

    fn root(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("elgar-shell-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn success_captures_stdout_stderr_exit_code_cwd_and_elapsed() {
        let root = root("success");
        let action = ShellCommandAction::new("printf 'hello'; printf 'note' >&2", &root);

        let result = execute_shell_command(&action).unwrap();

        assert_eq!(result.command, "printf 'hello'; printf 'note' >&2");
        assert_eq!(result.cwd, root.display().to_string());
        assert_eq!(result.stdout, "hello");
        assert_eq!(result.stderr, "note");
        assert!(!result.stdout_truncated);
        assert!(!result.stderr_truncated);
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
        assert!(result.elapsed_millis < 5_000);
    }

    #[test]
    fn nonzero_exit_captures_exit_code_and_output() {
        let root = root("nonzero");
        let action = ShellCommandAction::new("printf 'bad' >&2; exit 7", root);

        let result = execute_shell_command(&action).unwrap();

        assert_eq!(result.stdout, "");
        assert_eq!(result.stderr, "bad");
        assert!(!result.stdout_truncated);
        assert!(!result.stderr_truncated);
        assert_eq!(result.exit_code, Some(7));
        assert!(!result.timed_out);
    }

    #[test]
    fn timeout_kills_command_and_reports_timed_out() {
        let root = root("timeout");
        let mut action = ShellCommandAction::new("sleep 2", root);
        action.timeout_seconds = 0;

        let result = execute_shell_command(&action).unwrap();

        assert_eq!(result.exit_code, None);
        assert!(result.timed_out);
        assert!(result.elapsed_millis < 1_000);
    }

    #[test]
    fn output_caps_limit_stdout_and_stderr() {
        let root = root("caps");
        let mut action = ShellCommandAction::new("printf 'abcdef'; printf 'uvwxyz' >&2", root);
        action.output_caps = ShellCommandOutputCaps {
            stdout_bytes: 3,
            stderr_bytes: 4,
        };

        let result = execute_shell_command(&action).unwrap();

        assert_eq!(result.stdout, "abc");
        assert_eq!(result.stderr, "uvwx");
        assert!(result.stdout_truncated);
        assert!(result.stderr_truncated);
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
    }

    #[test]
    fn executor_returns_verified_shell_result() {
        let root = root("executor");
        let action = ShellCommandAction::new("printf 'ok'", root);

        let result = ShellExecutor::execute(&action).unwrap();

        assert!(matches!(
            result,
            crate::event::VerifiedActionResult::Shell(shell) if shell.stdout == "ok"
        ));
    }
}
