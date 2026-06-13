# Harness Loop Control

Coordinates one bounded harness loop.

## Files

- `mod.rs` exposes the loop entry point.
- `coordinator.rs` owns loop order only.
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

The coordinator handles provider `EmptyResponse` as a recoverable loop event:
retry once with generic runtime feedback, then synthesize from verified evidence
if the provider is still empty. If no evidence exists after the retry, the
provider error remains fatal.
