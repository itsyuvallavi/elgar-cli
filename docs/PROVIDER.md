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

## Harness Provider Contract

Normal CLI/TUI prompts go through the harness. The active provider path is
OpenAI-compatible chat with native tool schemas for executable read-only
primitives.

The normal successful flow is:

```text
provider returns native tool_calls
-> Elgar validates and executes read-only primitives
-> Elgar sends verified role:"tool" results
-> provider returns final text
```

JSON model-choice parsing and no-tool synthesis are fallback paths, not the
normal successful route.

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

Active harness modes use OpenAI-compatible chat, giving provider behavior one
supported LM Studio HTTP path.
Permissioned shell/write/edit execution is not active yet.
