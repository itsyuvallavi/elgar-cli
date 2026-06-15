# Provider

This folder owns Elgar's boundary with model providers.

It answers one question: how does Elgar turn internal chat data into a provider
request, send it, and turn the provider response back into Elgar data?

## Current Flow

```text
harness
  -> ControllerProvider trait
  -> ProviderCancelToken for cooperative cancellation
  -> LmStudioProvider
  -> provider config chooses profile
  -> lm_studio/openai.rs
  -> http/
  -> lm_studio/parse.rs
  -> ProviderOutput
```

Active harness request modes use OpenAI-compatible chat to keep one provider
route.

Provider calls accept a `ProviderCancelToken` on the cancelable trait methods.
The TUI uses that token for `/cancel`, and transports must poll it while waiting
on blocking reads.

## Folders

- `types/` - shared provider vocabulary:
  messages, requests, metadata, backend profiles, stream chunks, errors, and
  the `ControllerProvider` trait.

- `config/` - provider settings:
  local endpoint, model name, timeouts, compatibility flags, and named request
  profiles.

- `lm_studio/` - live LM Studio provider:
  formats requests, sends OpenAI-compatible calls for active harness modes,
  parses responses, and exposes `LmStudioProvider`.

- `http/` - tiny local HTTP helper:
  parses localhost URLs, opens TCP connections, writes JSON POST requests, reads
  normal/streaming responses, and decodes chunked bodies.

- `stub/` - no-network provider:
  deterministic provider used by tests and local harness checks.

## Root File

- `mod.rs` re-exports the provider surface used by the rest of Elgar.

Most code outside this folder should import from `provider::...` instead of
reaching into submodules directly.

## What Belongs Here

- provider request/response types
- cooperative provider cancellation
- provider configuration
- model backend selection
- LM Studio request/response handling
- local provider HTTP transport
- no-network provider test support

## What Does Not Belong Here

- TUI rendering
- session storage
- slash commands
- chat turn orchestration
- tool execution
- file or shell actions

## Suggested Reading Order

1. `types/README.md`
2. `config/README.md`
3. `lm_studio/README.md`
4. `http/README.md`
5. `stub/README.md`

## Checks

```text
cargo check -p elgar-core
cargo check -p elgar-tui
cargo check -p elgar-cli
```
