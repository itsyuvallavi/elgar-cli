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

Evidence must come from Rust collectors only. Provider prose is not verified
evidence. Native tool results send bounded verified evidence back to the model
as `role:"tool"` messages. Full evidence remains available for logs, details,
and fallback synthesis; compact summaries are no longer the primary model
protocol.
