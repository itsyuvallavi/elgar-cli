# terminal/display_context

## Purpose

This folder builds the live display context used by the terminal UI.

## Files

- `mod.rs` contains `TerminalShellContext` and helpers for footer, startup, and prompt display state.

## Difference From Startup

`display_context` gathers current display data: project path, cwd, provider name,
model name, metrics, context-window information, and pending approval footer
state.

`startup` renders the first visible startup text block.
