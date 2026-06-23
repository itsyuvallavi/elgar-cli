# elgar-cli/src/diagnostics

## Purpose

Diagnostic and scripted CLI surfaces.

## Files

- `mod.rs` registers and re-exports diagnostic modules.
- `logs.rs` reads existing system JSONL logs and renders the latest turn
  summary or follows live request timing for humans.
- `provider_smoke.rs` sends one direct LM Studio smoke-test request.
- `scripted_tui.rs` runs the line-based stdin/stdout TUI used by tests and
  scripts. It reuses `elgar-tui` slash-command parsing so scripted and
  interactive command names stay aligned.

## Ownership

Keep diagnostic commands explicit and small. They should help verify Elgar, not
become the normal chat runtime.

## Commands

- `elgar logs latest` prints the newest available `turn_perf_summary` from
  `.elgar/log/system`, skipping diagnostic logs that do not contain one.
- `elgar logs --follow` tails `.elgar/log/system` and prints live request
  lifecycle lines while another terminal runs Elgar.
