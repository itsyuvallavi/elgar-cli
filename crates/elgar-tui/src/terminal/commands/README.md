# elgar-tui/src/terminal/commands

## Purpose

Local slash-command support for the terminal UI.

This folder does not talk to the model. It only parses local commands and
provides helpers for local UI actions like copy, clear, and help text.

## Files

- `mod.rs` defines `TerminalCommand` and re-exports the command helpers.
- `parse.rs` turns submitted text into a `TerminalCommand`.
- `messages.rs` stores help, usage, and unknown-command text.
- `clipboard.rs` handles `/copy` and `/copy raw`.
- `clear.rs` handles `/clear` and visible terminal clearing.

## Boundary

Command execution is decided by `terminal/turn/submitted.rs`.

Example:

```text
/copy
  -> parse.rs returns TerminalCommand::Copy
  -> turn/submitted.rs decides to run copy
  -> clipboard.rs performs the copy
```
