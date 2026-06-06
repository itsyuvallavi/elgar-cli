# startup

## Purpose

This folder builds the first status block shown when the TUI starts.

## Files

- `mod.rs` contains `StartupBlock`, the small data structure rendered into startup text.
- `tests/` contains startup rendering tests.

## Difference From Display Context

`startup` renders the opening text block.

`terminal/display_context/` builds the live display context used by startup, footer, and prompt rendering.
