# terminal/ui

## Purpose

This folder owns terminal display and formatting.

## Files

- `mod.rs` exposes the UI modules to the parent terminal module.
- `approval.rs` renders core-owned pending approval state for risky primitives.
- `prompt.rs` draws the editable prompt and live provider preview.
- `footer.rs` formats the footer location/model/context line.
- `render.rs` renders conversation output and Ratatui frames.
- `text.rs` wraps and formats transcript text for printing.

## Rule

UI code should draw state it receives. It should not decide slash commands or start provider requests.
