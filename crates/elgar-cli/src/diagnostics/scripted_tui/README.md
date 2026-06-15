# diagnostics/scripted_tui

## Purpose

Line-based scripted TUI mode for tests and dogfood scripts.

Complex dogfoods can submit one multiline model prompt by framing it with
`/prompt` and `/end` on their own lines:

```text
/prompt
Create a complete app.
Requirements:
- include README.md
/end
/exit
```

Lines inside the block are joined with newlines and submitted as one harness
turn. Outside a prompt block, this mode remains line-based.

## Files

- `mod.rs` owns the stdin/stdout loop and runtime provider dispatch.
- `commands.rs` wraps local slash-command parsing for scripted mode.
- `render.rs` renders transcript output and pending approval blocks.

## Rule

Keep this mode deterministic. The real interactive TUI lives in `elgar-tui`.
