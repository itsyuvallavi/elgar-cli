# diagnostics/scripted_tui

## Purpose

Line-based scripted TUI mode for tests and dogfood scripts.

## Files

- `mod.rs` owns the stdin/stdout loop and runtime provider dispatch.
- `commands.rs` wraps local slash-command parsing for scripted mode.
- `render.rs` renders transcript output and pending approval blocks.

## Rule

Keep this mode line-based and deterministic. The real interactive TUI lives in
`elgar-tui`.
