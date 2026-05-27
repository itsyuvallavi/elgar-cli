# elgar-core/src/provider

## Purpose

Provider boundary for LM Studio and OpenAI-compatible chat behavior.

## Important Files

- `config.rs` defines provider configuration.
- `types.rs` defines chat messages and provider result types.
- `lm_studio.rs`, `lm_studio_format.rs`, and `lm_studio_parse.rs` handle LM Studio requests and responses.
- `http.rs` contains HTTP transport details.
- `stub.rs` supports deterministic tests.
- `mod.rs` exports the provider surface.

## Ownership

Keep HTTP and provider compatibility details here. Runtime and controller
compatibility code should consume typed provider results instead of parsing raw
provider text.

## Checks

- `cargo test -p elgar-core provider`
- `cargo run -p elgar-cli -- provider-smoke "Say hello in one sentence."`
