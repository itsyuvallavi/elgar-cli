# markdown

## Purpose

This folder renders assistant markdown into terminal-friendly text before it is
shown in conversation panes.

## Files

- `mod.rs` owns the public markdown rendering API used by the rest of the TUI.
- `normalize.rs` cleans small provider formatting artifacts before rendering.
- `inline.rs` renders inline emphasis while preserving paths and code spans.
- `lists.rs` renders list and preformatted lines.
- `tables.rs` renders markdown tables as aligned terminal text.
- `code.rs` bridges fenced markdown code blocks to `code_blocks`.
- `tests/markdown_test.rs` tests plain text, code blocks, lists, tables, hidden raw details, and preformatted text.
