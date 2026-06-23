# Provider Stub

This folder holds Elgar's no-network provider.

The stub is active support code for tests and local harness checks. It is not
the live LM Studio provider.

## Files

- `mod.rs` - deterministic provider implementation.
- `tests.rs` - tests for the deterministic stub behavior.

## Why It Exists

The stub lets Elgar exercise provider-facing code without opening a socket or
requiring LM Studio to be running.
