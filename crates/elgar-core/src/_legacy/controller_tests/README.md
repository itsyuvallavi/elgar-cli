# elgar-core/src/controller_tests

## Purpose

Focused unit tests for the remaining compatibility controller, typed action
lifecycle behavior, and provider-facing error handling.

## Important Files

- `basic_turns.rs` covers provider chat and local slash feedback.
- `action_lifecycle.rs` covers typed proposed, approved, rejected, and applied actions through `ActionGate`.
- `provider_streaming_errors.rs` covers provider-facing edge cases without mutating action truth.

## Checks

- `cargo test -p elgar-core --lib controller::tests`
