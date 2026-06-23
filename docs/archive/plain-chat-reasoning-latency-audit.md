# Plain Chat Reasoning Latency Audit

Date: 2026-06-03

## Scope

Audit why trivial/plain chat can still be slow after the tool-loop
optimization pass.

This audit does not change runtime validation, action truth, model routing, or
tool-enabled behavior. It checks whether the remaining latency is caused by:

- Elgar sending the wrong request payload
- LM Studio ignoring or rejecting reasoning controls
- Qwen reasoning heavily because of the prompt
- missing streaming for no-tool responses

## Baseline Symptom

Live TUI prompt:

```text
hello!
```

Latest Elgar trace:

```text
route: chat
provider_requests: 1
actions: 0
tool_calls: 0
request_mode: plain_chat
backend: lm_studio_native_chat
duration_ms: 21195
prompt_tokens: 232
completion_tokens: 875
reasoning_tokens: 862
request_bytes: 1023
```

This confirms the harness is not looping tools for plain chat. The latency is
inside the single provider request.

## Payload Findings

Current `elgar-provider.json`:

```json
{
  "request_modes": {
    "plain_chat": {
      "backend": "lm_studio_native_chat",
      "stats": true
    }
  }
}
```

Important: the current config does not set `reasoning: off`.

The native LM Studio request path can serialize `reasoning` when the request
profile includes it, but the current checked-in config does not request it.

## Direct LM Studio Results

Endpoint:

```text
POST http://127.0.0.1:1234/api/v1/chat
```

Model:

```text
qwen3.6-35b-a3b-ud-mlx
```

### Tiny System Prompt

Payload:

```json
{
  "model": "qwen3.6-35b-a3b-ud-mlx",
  "input": "hello!",
  "system_prompt": "Answer briefly in one sentence.",
  "stream": false
}
```

Result:

```text
duration: 4.1s
input_tokens: 23
total_output_tokens: 147
reasoning_output_tokens: 135
time_to_first_token_seconds: 0.793
```

### Exact Elgar System Prompt

Payload used the same prompt Elgar currently sends for plain chat.

Result:

```text
duration: 22.8s
input_tokens: 104
total_output_tokens: 1173
reasoning_output_tokens: 1100
time_to_first_token_seconds: 1.018
```

This reproduces the Elgar latency outside Elgar. The exact prompt causes Qwen to
spend a long time checking constraints before answering a trivial greeting.

### Compressed Safety Prompt

Payload:

```json
{
  "model": "qwen3.6-35b-a3b-ud-mlx",
  "input": "hello!",
  "system_prompt": "You are Elgar. Reply briefly in terminal-friendly prose. Do not claim files changed or commands ran unless the transcript says they were verified.",
  "stream": false
}
```

Result:

```text
duration: 11.5s
total_output_tokens: 462
reasoning_output_tokens: 447
```

Prompt compression helps, but the model still reasons heavily.

### Minimal Prompt

Payload:

```json
{
  "model": "qwen3.6-35b-a3b-ud-mlx",
  "input": "hello!",
  "system_prompt": "You are Elgar. Reply briefly.",
  "stream": false
}
```

Result:

```text
duration: 7.5s
total_output_tokens: 265
reasoning_output_tokens: 254
```

### Casual Greeting Prompt

Payload:

```json
{
  "model": "qwen3.6-35b-a3b-ud-mlx",
  "input": "hello!",
  "system_prompt": "You are Elgar. For casual greetings, answer with a short friendly sentence. For real tasks, be concise and accurate.",
  "stream": false
}
```

Result:

```text
duration: 6.4s
total_output_tokens: 230
reasoning_output_tokens: 216
time_to_first_token_seconds: 0.577
```

This was the fastest prompt variant tested, but it is still slower than a
non-reasoning model should be for a greeting.

## Reasoning Control Result

Direct native LM Studio calls with `reasoning` set to `off`, `low`, or `high`
all failed:

```text
Model 'qwen3.6-35b-a3b-ud-mlx' does not expose reasoning configuration.
```

So forcing `reasoning: off` is not a safe fix for this loaded Qwen model. Elgar
would need a provider capability check or fallback before sending that field.

## Streaming Result

Native LM Studio streaming is supported:

```json
{
  "stream": true
}
```

The stream emits events such as:

```text
chat.start
prompt_processing.start
reasoning.start
reasoning.delta
reasoning.end
message.start
message.delta
message.end
chat.end
```

For this Qwen model, reasoning deltas arrive before message deltas. Streaming
therefore improves perceived progress, but visible answer text still waits until
reasoning finishes.

## Verdict

The current slow `hello!` path is not caused by tool loops.

Confirmed:

- Elgar uses one `plain_chat` provider request.
- No tools are exposed.
- No actions are created.
- The backend is `lm_studio_native_chat`.
- LM Studio rejects explicit reasoning controls for this Qwen model.
- The exact Elgar prompt reproduces the 20s class latency directly in LM Studio.

Primary bottleneck:

```text
Prompt-induced hidden reasoning on qwen3.6-35b-a3b-ud-mlx.
```

## Recommended Next Fix

1. Add request-mode-specific prompt profiles.
   - Keep the strict current controller prompt for tool/action contexts.
   - Use a shorter prompt for `plain_chat`.
   - Preserve the verified-action safety rule in compressed form.

2. Add a provider capability guard for reasoning fields.
   - Do not send `reasoning` unless the model/backend is known to accept it.
   - If configured reasoning is rejected, record the provider error clearly and
     fall back only when safe.

3. Add native streaming support for no-tool responses.
   - Parse LM Studio native SSE events.
   - Show progress while reasoning streams.
   - Do not expose hidden reasoning in the completed transcript.

4. Add optional small-model routing.
   - Use a faster non-reasoning model for route JSON, state classifier,
     trivial/plain chat, and safe synthesis.
   - Keep Qwen for hard planning/review/tool reasoning if it performs better
     there.

## Non-Fixes

- Do not hardcode greeting responses.
- Do not cap output tokens as the primary fix.
- Do not force `reasoning: off` blindly for this Qwen model.
- Do not replace the verified shell/action truth path.
