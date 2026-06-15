# Harness Loop Evidence

Turns validated primitive requests into verified evidence for the model.

## Files

- `mod.rs` exposes evidence execution modules inside `harness_loop`.
- `execution.rs` runs executable primitive collectors.
- `keys.rs` builds stable duplicate/budget keys for primitive requests.
- `request_args.rs` reads typed arguments from validated primitive requests.
- `render.rs` renders verified evidence, permission evidence, and execution
  errors.
- `summary.rs` renders compact summaries for fallback decision/repair paths.
- `state.rs` measures full evidence bytes versus compact prompt bytes.
- `timeline.rs` renders a compact verified action timeline for later provider
  rounds and fallback synthesis.

Evidence must come from Rust collectors only. Provider prose is not verified
evidence. Native tool results send bounded verified evidence back to the model
as `role:"tool"` messages. Full evidence remains available for logs, details,
and fallback synthesis; compact summaries are no longer the primary model
protocol.

Tool-result messages also include a compact verified action timeline when the
turn has writes, edits, command runs, no-ops, or execution errors. The timeline
is generic: it records paths, command exit codes, and recovery order without
framework-specific rules. This keeps failed command -> fix -> passing rerun
sequences visible before the model writes final text. Logs record only compact
timeline metadata, not the full provider tool-result body.
