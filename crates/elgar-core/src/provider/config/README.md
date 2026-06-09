# Provider Config

This folder owns provider configuration.

It is data-only: it does not make provider requests, open sockets, or mutate
session state.

## Files

- `mod.rs` - provider config structs, defaults, and request-mode profile lookup.
- `tests.rs` - config defaults and deserialization tests.

## Main Ideas

- `ProviderConfig` stores endpoint, model, timeout, streaming, compatibility,
  and request-mode settings.
- `ProviderCompatibility` stores optional model/provider capability hints.
- `request_profile_for_mode` chooses backend options for a named mode. Active
  harness modes currently use OpenAI-compatible chat.
