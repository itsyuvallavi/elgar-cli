# elgar-core/src/provider/lm_studio

## Purpose

LM Studio provider implementation. This folder keeps LM Studio-specific request
formatting, response parsing, and OpenAI-compatible calls out of the provider
root.

## Files

- `mod.rs` owns public LM Studio helper exports and request id generation.
- `backend.rs` owns `LmStudioProvider` and its `ControllerProvider`
  implementation.
- `openai.rs` sends OpenAI-compatible LM Studio chat requests, tool requests,
  and streaming requests.
- `openai/` contains OpenAI-compatible request metrics, timeout helpers, and
  streaming response assembly.
- `format.rs` builds OpenAI-compatible chat request structs and JSON bodies.
- `parse.rs` parses OpenAI-compatible, streaming, and error responses into
  Elgar `ProviderOutput` and `ProviderError` values.
- `tests/` contains active LM Studio provider tests split by request
  formatting, response parsing, and local HTTP behavior.

## Ownership

- Keep provider selection and trait implementation in `backend.rs`.
- Keep public helper exports and request ids in `mod.rs`.
- Keep active harness HTTP execution in `openai.rs`.
- Keep request JSON construction in `format.rs`.
- Keep response JSON parsing in `parse.rs`.

The rest of Elgar should call `LmStudioProvider` or public provider helpers
instead of reaching into these files directly.

## Checks

- `cargo check -p elgar-core`
- `cargo check -p elgar-cli`
- `cargo test -p elgar-core provider::lm_studio -- --nocapture`
