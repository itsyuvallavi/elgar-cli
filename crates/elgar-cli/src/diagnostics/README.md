# elgar-cli/src/diagnostics

## Purpose

Diagnostic and scripted CLI surfaces.

## Files

- `mod.rs` registers and re-exports diagnostic modules.
- `provider_smoke.rs` sends one direct LM Studio smoke-test request.
- `scripted_tui.rs` runs the line-based stdin/stdout TUI used by tests and scripts.

## Ownership

Keep diagnostic commands explicit and small. They should help verify Elgar, not
become the normal chat runtime.
