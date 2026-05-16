# TUI Visual QA Checklist

Use this checklist before committing terminal TUI polish.

Visual target: `docs/pi-like-terminal-tui-visual-spec.md`.

Run:

```sh
cargo run -p elgar-cli -- tui-terminal
```

Review these states:

- Fresh open.
- After one prompt.
- While thinking or loading.
- After response.
- Narrow-ish terminal.
- Wide terminal.
- Long response with scrolling.
- Text selection still works.

Before commit:

- Capture or review screenshots for the changed states.
- Confirm startup has breathing room, one clear sentence, real context files, and `provider · model`.
- Reject noisy footer hints.
- Reject label-heavy chat transcripts.
- Reject fake Skills, MCP, Bash, API, settings, or unimplemented capability sections.
- Confirm normal terminal text selection remains usable.
