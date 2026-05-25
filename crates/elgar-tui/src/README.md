# elgar-tui/src

## Purpose

Source modules for the terminal UI shell and renderable TUI surfaces.

## Important Files and Folders

- `layout.rs`, `theme.rs`, and `markdown.rs` shape visual output.
- `action_panel.rs`, `panes.rs`, and `panes/` render session areas.
- `terminal.rs` and `terminal/` own the interactive terminal loop.
- `smoke.rs` supports deterministic smoke rendering.
- `lib.rs` exports the public TUI surface.

## Ownership

Render state from core and keep UI-only state local. Escalate policy or truth changes back to `elgar-core`.

## Checks

- `cargo test -p elgar-tui`
- `cargo test -p elgar-tui terminal`
