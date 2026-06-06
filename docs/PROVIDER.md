# Provider

## Purpose

Elgar currently talks to LM Studio through local provider config.

Config file:

```text
elgar-provider.json
```

## Current Shape

Example:

```json
{
  "provider": "lm-studio",
  "base_url": "http://127.0.0.1:1234/v1",
  "default_model": "qwen3.6-35b-a3b-mlx",
  "mode": "live",
  "stream": true
}
```

If provider config is disabled or missing, Elgar uses the deterministic stub
provider for tests.

Disable live provider:

```sh
ELGAR_PROVIDER_CONFIG=off elgar
```

## Raw Chat Contract

Plain chat should send one no-tool provider request.

It should not:

- attach tools
- send `tool_choice`
- inject project memory
- run folder anchoring
- make a follow-up synthesis request

## Diagnostics

Provider smoke command:

```sh
elgar provider-smoke "Say hello in one sentence."
```

Required environment variable for smoke mode:

```sh
ELGAR_LM_STUDIO_MODEL
```

Optional:

```sh
ELGAR_LM_STUDIO_BASE_URL
```

## Notes

LM Studio can expose different backend formats. Current raw chat may use native
or OpenAI-compatible no-tool chat depending on provider config. Tool-capable
provider code can exist, but tool execution is not active in the raw baseline.
