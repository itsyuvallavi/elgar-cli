# Loop Flow Tests

Tests for the primitive harness loop: decision calls, evidence collection,
repair behavior, synthesis, and stop reasons.

## Files

- `mod.rs` wires the loop-flow test modules.
- `loop_helpers.rs` holds shared queued-provider and tool-result helpers.
- `native_loop_test.rs` covers native provider tool-call turns.
- `permission_loop_test.rs` covers risky primitive approval evidence.
- `memory_loop_test.rs` covers duplicate request and same-turn memory behavior.
- `batch_loop_test.rs` covers batched requests, evidence budgets, and
  synthesis triggers.
- `final_text_loop_test.rs` covers direct final text after zero or more tool
  results.
- `repair_loop_test.rs` covers invalid model-choice repair and safe fallback.
- `execution_failure_loop_test.rs` covers verified execution errors returned as
  tool results.
