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
- Confirm `tui-terminal` behaves as an inline terminal transcript, not a fixed full-screen panel.
- Confirm startup has breathing room, one clear sentence, real context files, and `provider · model`.
- Confirm the active prompt marker is on the input row and no cursor/block appears below the footer.
- Confirm the prompt frame has matching separators above and below the input row.
- Reject noisy footer hints.
- Reject a large empty fixed conversation panel between the transcript and footer.
- Reject label-heavy chat transcripts.
- Reject fake Skills, MCP, Bash, API, settings, or unimplemented capability sections.
- Confirm normal terminal text selection remains usable.
