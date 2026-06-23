//! Clipboard helpers for `/copy` terminal commands.
//!
//! This copies either the visible conversation or raw hidden details.

use std::io::{self, Write};
#[cfg(any(test, all(not(test), target_os = "macos")))]
#[cfg(all(not(test), target_os = "macos"))]
use std::process::{Command, Stdio};

use crate::TuiShell;

pub(crate) fn copy_conversation_to_terminal_clipboard(
    writer: impl Write,
    shell: &mut TuiShell,
) -> io::Result<()> {
    copy_conversation_with_clipboards(writer, shell, copy_text_to_system_clipboard)
}

pub(super) fn copy_conversation_with_clipboards(
    mut writer: impl Write,
    shell: &mut TuiShell,
    system_clipboard: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    let text = shell.conversation_copy_text();

    copy_text_with_clipboards(&mut writer, shell, &text, "conversation", system_clipboard)
}

pub(crate) fn copy_raw_details_to_terminal_clipboard(
    writer: impl Write,
    shell: &mut TuiShell,
) -> io::Result<()> {
    copy_raw_details_with_clipboards(writer, shell, copy_text_to_system_clipboard)
}

pub(super) fn copy_raw_details_with_clipboards(
    writer: impl Write,
    shell: &mut TuiShell,
    system_clipboard: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    let Some(text) = shell.raw_details_copy_text() else {
        let message = "no raw details available".to_string();
        shell.copy.mark_failed(message.clone());
        return Err(io::Error::new(io::ErrorKind::NotFound, message));
    };

    copy_text_with_clipboards(writer, shell, &text, "raw details", system_clipboard)
}

fn copy_text_with_clipboards(
    mut writer: impl Write,
    shell: &mut TuiShell,
    text: &str,
    copied_item: &str,
    system_clipboard: impl FnOnce(&str) -> io::Result<()>,
) -> io::Result<()> {
    match system_clipboard(text) {
        Ok(()) => {
            mark_copy_success(shell, copied_item, text.len());
            Ok(())
        }
        Err(system_error) => match copy_text_with_osc52(&mut writer, text) {
            Ok(()) => {
                mark_copy_success(shell, copied_item, text.len());
                Ok(())
            }
            Err(terminal_error) => {
                let message = format!(
                    "system clipboard failed: {system_error}; terminal fallback failed: {terminal_error}"
                );
                shell.copy.mark_failed(message.clone());
                Err(io::Error::new(terminal_error.kind(), message))
            }
        },
    }
}

fn mark_copy_success(shell: &mut TuiShell, copied_item: &str, bytes: usize) {
    if copied_item == "conversation" {
        shell.copy.mark_copied(bytes);
    } else {
        shell.copy.mark_copied_item(copied_item, bytes);
    }
}

fn copy_text_with_osc52(mut writer: impl Write, text: &str) -> io::Result<()> {
    writer.write_all(osc52_clipboard_sequence(text).as_bytes())?;
    writer.flush()
}

#[cfg(not(test))]
fn copy_text_to_system_clipboard(text: &str) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        copy_text_with_command("/usr/bin/pbcopy", text)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = text;
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no local system clipboard command configured",
        ))
    }
}

#[cfg(test)]
fn copy_text_to_system_clipboard(_text: &str) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        "system clipboard disabled in unit tests",
    ))
}

#[cfg(all(not(test), target_os = "macos"))]
fn copy_text_with_command(command: &str, text: &str) -> io::Result<()> {
    copy_text_with_command_and_args(command, &[], text, std::time::Duration::from_millis(1_500))
}

#[cfg(all(not(test), target_os = "macos"))]
fn copy_text_with_command_and_args(
    command: &str,
    args: &[&str],
    text: &str,
    timeout: std::time::Duration,
) -> io::Result<()> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let mut stdin = child.stdin.take().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::BrokenPipe,
            "clipboard command did not open stdin",
        )
    })?;
    let text = text.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        stdin.write_all(&text)?;
        drop(stdin);
        io::Result::Ok(())
    });

    let started = std::time::Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "clipboard command timed out",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };

    writer
        .join()
        .map_err(|_| io::Error::other("clipboard writer thread panicked"))??;

    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "clipboard command exited with {status}"
        )))
    }
}

pub(super) fn osc52_clipboard_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", encode_base64(text.as_bytes()))
}

pub(super) fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        let triple = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);

        encoded.push(TABLE[((triple >> 18) & 0x3f) as usize] as char);
        encoded.push(TABLE[((triple >> 12) & 0x3f) as usize] as char);

        if chunk.len() > 1 {
            encoded.push(TABLE[((triple >> 6) & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }

        if chunk.len() > 2 {
            encoded.push(TABLE[(triple & 0x3f) as usize] as char);
        } else {
            encoded.push('=');
        }
    }

    encoded
}
