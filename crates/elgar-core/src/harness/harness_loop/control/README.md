# Harness Loop Control

Coordinates one bounded harness loop.

## Files

- `mod.rs` exposes the loop entry point.
- `entrypoint.rs` owns public wrapper functions for cancellation/no-stream
  callers.
- `coordinator.rs` owns loop order only.
- `loop_setup.rs` initializes one loop's registry, budget, memory, and initial
  messages.
- `model_text_round.rs` handles provider prose claim checks and final text
  stops.
- `choice_from_output.rs` converts provider text/tool calls into `ModelChoice`.
- `request_handling.rs` executes validated primitive requests.
- `native_execution.rs` bridges one native tool request into verified evidence
  plus a matching tool-result message.
- `provider_error.rs` owns transient provider-error recovery.
- `synthetic_tool_calls.rs` builds provider-shaped tool calls for JSON fallback
  requests.
- `finish.rs` owns final message and synthesis finish paths.
- `start.rs` logs loop startup metadata.

Keep provider HTTP calls, primitive collectors, and shared result types out of
this folder.

## Provider Recovery

`provider_error.rs` handles provider `EmptyResponse` as a recoverable loop event:
retry once with generic runtime feedback, then synthesize from verified evidence
if the provider is still empty. If no evidence exists after the retry, the
provider error remains fatal.

## Approval Stops

When one risky request needs approval, the loop records the pending approval and
stops immediately with `approval_pending`. This prevents later provider prose
from implying that more actions are approved than the runtime actually queued.
Risky batches remain supported when the provider emits multiple risky tool calls
in the same response.
