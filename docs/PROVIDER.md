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

## Streaming

LM Studio/OpenAI-compatible chat supports streaming response chunks. Active
harness provider calls stream reasoning and text chunks when the provider sends
them, while typed tool calls are still executed only after the full provider
response has been parsed and validated.

For the current OpenAI-compatible `/v1/chat/completions` path, Elgar treats the
SSE `data: [DONE]` sentinel as provider completion and stops reading the
response body at that point. It does not wait for a delayed TCP/socket close.
The native LM Studio REST streaming API uses named events instead; if Elgar
adds that path, the equivalent completion signal is `chat.end`.

For tool-enabled calls, streamed tool-call deltas are assembled into complete
native tool calls before the harness executes anything. Partial streamed tool
JSON is never executable runtime truth.

Tool-enabled OpenAI-compatible requests send the available tool schemas but do
not send `tool_choice`. The model remains free to call a tool or finish with
normal text, while Rust validates every returned tool call before execution.

Request profiles may set `stats: true`; Elgar serializes that flag into
OpenAI-compatible chat requests. For streaming requests, Elgar also sends the
standard `stream_options.include_usage` request option and records
provider-reported usage when LM Studio includes usage in the final response or
streaming usage chunk. The TUI uses only those provider-reported token counts
for response token summaries and real context-window percentages.

Reasoning controls are opt-in provider compatibility settings. Elgar does not
use TUI truncation as a provider-control substitute: full reasoning is logged
for diagnostics, while normal chat shows compact readable reasoning text. If
the local provider/model supports a reasoning request shape, configure it
explicitly:

```json
{
  "compatibility": {
    "reasoning": {
      "request_format": "qwen_chat_template"
    }
  },
  "request_modes": {
    "harness_tool_decision": {
      "reasoning": "minimal",
      "stats": true
    }
  }
}
```

Supported `request_format` values are:

- `reasoning_effort`: sends OpenAI-style `reasoning_effort`.
- `qwen_enable_thinking`: sends top-level `enable_thinking`.
- `qwen_chat_template`: sends `chat_template_kwargs.enable_thinking` and
  `preserve_thinking`.

If `request_format` is omitted, Elgar keeps the current safe behavior and sends
no reasoning-control field.

Session JSONL records the requested reasoning level, reasoning output tokens
when the provider reports them, and reasoning output chars. Normal chat should
render compact reasoning text, not diagnostic metadata counts.

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
