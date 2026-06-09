# Harness Loop Control

Coordinates one bounded harness loop.

## Files

- `mod.rs` exposes the loop entry point.
- `coordinator.rs` owns loop order only.
- `choice_from_output.rs` converts provider text/tool calls into `ModelChoice`.
- `request_handling.rs` executes validated primitive requests.
- `finish.rs` owns final message and synthesis finish paths.
- `start.rs` logs loop startup metadata.

Keep provider HTTP calls, primitive collectors, and shared result types out of
this folder.
