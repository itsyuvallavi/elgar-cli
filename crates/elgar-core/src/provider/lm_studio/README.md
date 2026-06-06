# elgar-core/src/provider/lm_studio

## Purpose

LM Studio provider implementation. This folder keeps LM Studio-specific request
formatting, response parsing, native backend calls, and OpenAI-compatible calls
out of the provider root.

## Files

- `mod.rs` owns `LmStudioProvider`, public LM Studio helper exports, request id
  generation, and routing between native and OpenAI-compatible helper modules.
- `openai.rs` sends OpenAI-compatible LM Studio chat requests, tool requests,
  and streaming requests. It also records OpenAI-compatible request metrics.
- `native.rs` sends native LM Studio no-tool chat requests and builds the native
  `/api/v1/chat` request body.
- `format.rs` builds OpenAI-compatible chat request structs and JSON bodies.
- `parse.rs` parses OpenAI-compatible, streaming, native, and error responses
  into Elgar `ProviderOutput` and `ProviderError` values.

## Ownership

- Keep provider selection in `mod.rs`.
- Keep HTTP execution in `openai.rs` and `native.rs`.
- Keep request JSON construction in `format.rs`.
- Keep response JSON parsing in `parse.rs`.

The rest of Elgar should call `LmStudioProvider` or public provider helpers
instead of reaching into these files directly.

## Checks

- `cargo check -p elgar-core`
- `cargo check -p elgar-cli`
