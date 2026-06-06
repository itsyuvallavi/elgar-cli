# elgar-tui/src

## Purpose

Source modules for the terminal UI shell and renderable TUI surfaces.

## Important Files and Folders

- `lib.rs` exports the public TUI surface.
- `shell.rs` holds the minimal TUI state and applies core events to panes.
- `input.rs` owns the editable text buffer used by the terminal prompt.
- `layout.rs` names logical TUI regions and renders simple text sections.
- `theme.rs` stores Ratatui color/style choices.
- `markdown.rs` renders assistant markdown into terminal-friendly text.
- `code_blocks.rs` formats fenced code/script blocks into boxed terminal output.
- `panes.rs` exports pane types from `panes/`.
- `panes/` stores conversation, status, copy, event-rendering, and provider-reasoning panes.
- `startup/` builds the opening startup text block.
- `terminal.rs` and `terminal/` own the interactive terminal loop.
- `_legacy/` stores archived old tool/action UI files and stale tests.

## Ownership

Render state from core and keep UI-only state local. Escalate policy or truth changes back to `elgar-core`.

## Checks

- `cargo test -p elgar-tui`
- `cargo test -p elgar-tui --lib`
