# LM Studio Smart Integration Plan

Status: planning only.

This plan is separate from `docs/transactional-shell-synthesis-implementation-plan.md`.
Transactional shell synthesis reduces repeated tool/model rounds. This plan
targets the cost of each provider call, especially the slow one-request
`hello!` case where hidden model reasoning dominates.

## Goal

Make Elgar faster against local LM Studio models while preserving the active
Elgar contract:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

The goal is not to make the harness looser. The goal is to make provider calls
better shaped:

- use lower reasoning where the runtime only needs route/state JSON
- use LM Studio native no-tool APIs where they help
- keep custom tool turns on the proven tool-capable path
- collect real timing/token stats from LM Studio
- keep visible assistant replies provider-authored
- keep plain chat plain first

## Source Facts

As of June 2, 2026, LM Studio documents:

- OpenAI-compatible `/v1/chat/completions`
- OpenAI-compatible `/v1/responses`
- LM Studio native `/api/v1/chat/completions`
- LM Studio native `/api/v1/responses`
- LM Studio native `/api/v1/chat`
- request fields such as `reasoning`, `context_length`, `stream`, `stats`,
  and `previous_response_id` on the native chat surface

Official references:

- <https://lmstudio.ai/docs/developer/rest>
- <https://lmstudio.ai/docs/developer/rest/chat>

Important constraint: the native `/api/v1/chat` surface is useful for
stateful/no-tool chat, but it should not be assumed to replace Elgar's custom
typed tool-call path. Keep `tool_enabled` on the current tool-capable API until
`/v1/responses` or another endpoint is proven compatible with Elgar's tool
schema and parser.

## Current Elgar State

Elgar currently uses an LM Studio/OpenAI-compatible provider shape:

- `crates/elgar-core/src/provider/config.rs`
  - default base URL is `http://127.0.0.1:1234/v1`
  - request URL is `{base_url}/chat/completions`
  - config has `stream`, timeouts, context window, and parsing compatibility
  - config does not carry request-mode-specific reasoning or backend choices

- `crates/elgar-core/src/provider/lm_studio_format.rs`
  - formats `ChatRequest`
  - sends `model`, `messages`, `stream`, `temperature`, `tools`, `tool_choice`
  - does not send `reasoning`, `context_length`, `stats`, or
    `previous_response_id`

- `crates/elgar-core/src/provider/lm_studio.rs`
  - tool-enabled calls force `stream = false`
  - no-tool calls can stream only if config enables streaming
  - `chat_messages_without_streaming_with_metadata` forces non-streaming

- `crates/elgar-core/src/agent_request_mode.rs`
  - request modes exist:
    - `PlainChat`
    - `ToolEnabled`
    - `ToolResultSynthesis`
    - `ProjectReviewSynthesis`
  - `provider_request_metadata_for_mode` currently ignores the mode and only
    returns basic metadata

The current live evidence says `hello!` uses one `plain_chat` provider request,
zero tools, and provider-authored text, but can still take around 10 to 13
seconds on a reasoning-heavy local Qwen model. That means the next bottleneck
is model/provider generation behavior, not extra Elgar tool rounds.

## Non-Goals

- Do not add canned assistant replies for greetings or common prompts.
- Do not add natural-language trigger tables.
- Do not weaken runtime validation, policy, permissions, or executor checks.
- Do not use visible-answer output caps as a latency shortcut.
- Do not rewrite the provider stack around an internal model runtime yet.
- Do not make native LM Studio chat the default for tool-enabled turns until
  custom tool compatibility is proven.

## Recommended Architecture

Add a small provider request profile layer:

```text
AgentProviderRequestMode
        |
        v
ProviderRequestProfile
        |
        +-- backend preference
        +-- stream preference
        +-- reasoning preference
        +-- context length override
        +-- stats preference
        +-- stateful continuation preference
```

This keeps policy/runtime code out of provider details while allowing Elgar to
shape local provider calls by purpose.

### Backend Kinds

Start with explicit backend kinds:

```text
openai_chat_completions
lm_studio_native_chat
openai_responses_probe
```

Initial behavior:

- `openai_chat_completions`: current default and required fallback
- `lm_studio_native_chat`: opt-in for no-tool modes only
- `openai_responses_probe`: experimental, disabled by default

The provider should select the backend per request mode. Unsupported fields
must be ignored or rejected at config-load time with a clear message, not sent
blindly to every endpoint.

### Request Mode Defaults

Recommended first defaults:

```text
plain_chat:
  backend: openai_chat_completions by default
  optional backend: lm_studio_native_chat
  tools: never
  tool_choice: never
  reasoning: off or low when native endpoint supports it
  streaming: optional, only after the route JSON path is safe
  stats: enabled when native endpoint supports it

plain_state_classifier:
  backend: no-tool path
  tools: never
  reasoning: off or low
  stats: enabled when available

tool_enabled:
  backend: openai_chat_completions
  tools: enabled and intent-scoped
  streaming: false
  reasoning: configured default or medium
  stats: use whatever the endpoint returns

tool_result_synthesis:
  backend: no-tool path
  tools: never
  reasoning: low or medium
  streaming: optional later
  stats: enabled when available

project_review_synthesis:
  backend: no-tool path
  tools: never
  reasoning: low or medium
  streaming: optional later
  stats: enabled when available
```

Add `PlainStateClassifier` as an explicit request mode if needed. Today that
path is reported as `"plain_state_classifier"` in events but uses
`AgentProviderRequestMode::PlainChat` metadata.

## Implementation Plan

### Phase 0: Baseline And Acceptance Targets

No behavior change.

Capture current live baselines with the same loaded LM Studio model:

```text
hello!
/tokens
/reasoning
/exit
```

Also capture a direct LM Studio prompt with the closest equivalent prompt. The
direct prompt is not an exact apples-to-apples comparison, but it tells us what
the loaded model can do without Elgar's route contract.

Record:

- provider request count
- request modes
- serialized request bytes
- prompt/completion/total tokens
- visible chars
- thinking/reasoning chars
- first-token time when available
- total provider duration
- LM Studio tokens/sec when available
- prompt cache info when available

Use existing:

```sh
elgar perf-trace
./bin/perf-trace
```

Expected finding: plain chat already uses one request, so major wins must come
from reasoning control, streaming UX, native stats, or a faster model/router.

### Phase 1: Request Profile Types

Add data-only request profile types in provider/core code.

Likely files:

- `crates/elgar-core/src/agent_request_mode.rs`
- `crates/elgar-core/src/provider/config.rs`
- `crates/elgar-core/src/provider/types.rs`
- `crates/elgar-cli/src/provider_config.rs`
- `docs/provider-compatibility.md`

Suggested types:

```rust
enum ProviderBackendKind {
    OpenAiChatCompletions,
    LmStudioNativeChat,
    OpenAiResponsesProbe,
}

enum ProviderReasoningLevel {
    Off,
    Low,
    Medium,
    High,
}

struct ProviderRequestProfile {
    mode: AgentProviderRequestMode,
    backend: ProviderBackendKind,
    stream: Option<bool>,
    reasoning: Option<ProviderReasoningLevel>,
    context_length: Option<u64>,
    stats: Option<bool>,
    stateful: Option<bool>,
}
```

Keep defaults behavior-preserving:

- no new fields required in `elgar-provider.json`
- current `/v1/chat/completions` behavior stays default
- no request emits new LM Studio-specific fields unless the backend supports
  them and config opts in

### Phase 2: Native No-Tool LM Studio Client

Add a native no-tool client path for `/api/v1/chat`.

Likely files:

- `crates/elgar-core/src/provider/lm_studio.rs`
- `crates/elgar-core/src/provider/lm_studio_format.rs`
- `crates/elgar-core/src/provider/lm_studio_parse.rs`
- optional new file: `crates/elgar-core/src/provider/lm_studio_native.rs`

Rules:

- only use this path when `tools.is_empty()`
- never send `tool_choice`
- never use it for `tool_enabled`
- preserve exact provider-authored text handling
- parse text, reasoning, stats, and response IDs separately
- fall back to the current chat-completions path if the native path is not
  configured

The first supported modes should be:

- `PlainChat`
- `ToolResultSynthesis`
- `ProjectReviewSynthesis`
- `PlainStateClassifier` if added

### Phase 3: Per-Mode Reasoning Controls

Use LM Studio request-level reasoning where supported.

Do not treat reasoning as a global user toggle only. Elgar needs different
effort by job:

- route/state JSON: off or low
- simple visible chat: off or low
- synthesis over verified tool output: low or medium
- tool planning/execution: model default or medium/high when the user chooses

This is the main `hello!` latency lever. If the model is spending 10 seconds
in hidden reasoning for a greeting, the request should not ask for that level
of reasoning.

Add config with explicit opt-in:

```json
{
  "provider": "lm-studio",
  "base_url": "http://127.0.0.1:1234/v1",
  "model": "loaded-model-name",
  "request_modes": {
    "plain_chat": {
      "backend": "lm_studio_native_chat",
      "reasoning": "off",
      "stats": true
    },
    "plain_state_classifier": {
      "backend": "lm_studio_native_chat",
      "reasoning": "off",
      "stats": true
    },
    "tool_result_synthesis": {
      "backend": "lm_studio_native_chat",
      "reasoning": "low",
      "stats": true
    },
    "project_review_synthesis": {
      "backend": "lm_studio_native_chat",
      "reasoning": "low",
      "stats": true
    },
    "tool_enabled": {
      "backend": "openai_chat_completions"
    }
  }
}
```

The exact schema can change during implementation, but the semantics should
stay mode-specific and opt-in.

### Phase 4: Provider Stats And Observability

Parse and store native LM Studio stats when present.

Add fields to provider metrics if available:

- provider-reported time to first token
- provider-reported generation time
- provider-reported tokens per second
- prompt tokens
- completion tokens
- reasoning tokens if exposed
- stop reason
- response ID / previous response ID

Likely files:

- `crates/elgar-core/src/event.rs`
- `crates/elgar-core/src/provider/types.rs`
- `crates/elgar-core/src/provider/lm_studio_parse.rs`
- trace/perf summary files that already compute provider timing

Acceptance: `elgar perf-trace` should distinguish:

```text
elapsed wall time
provider HTTP duration measured by Elgar
provider generation time reported by LM Studio
first token latency
tokens/sec
reasoning/thinking size when available
```

This prevents guessing about whether the bottleneck is Elgar overhead, model
reasoning, prompt cache misses, or slow generation.

### Phase 5: Streaming UX For Safe No-Tool Modes

Streaming improves perceived speed even when total generation time is similar.

Apply only after native no-tool parsing is stable:

- stream `ToolResultSynthesis` and `ProjectReviewSynthesis` first
- consider streaming visible chat only when route/content framing is safe
- do not stream tool-enabled turns until tool-call parsing is robust
- keep `/cancel` semantics honest: cancellation may hide/drop updates before it
  aborts the socket

If `plain_chat` still uses a structured route JSON response, do not stream raw
partial JSON into the transcript. Either wait for the full route result or
split the path cleanly.

### Phase 6: Stateful Continuation Experiment

Evaluate `previous_response_id` only for no-tool modes.

Potential benefit:

- less repeated context sent over HTTP
- possible server-side state/cache reuse
- better direct LM Studio parity

Risk:

- provider-side state must never become Elgar truth
- verified files/actions/plans still come from Elgar session state
- replay/resume must not depend on opaque LM Studio state

Safe first experiment:

- store response IDs only in the live in-memory session
- use them only for no-tool chat/synthesis continuation
- disable across process restart
- never use them for verified state, tool execution, or resume truth

If the experiment does not clearly improve latency or token usage, do not keep
it.

### Phase 7: `/v1/responses` Probe For Tool Turns

Evaluate LM Studio/OpenAI-compatible `/v1/responses` separately.

Questions to answer before using it:

- Does it support Elgar's exact custom tool schema?
- Does it preserve enough tool-call IDs for Elgar's feedback loop?
- Does it expose better reasoning/stats controls than chat completions?
- Does it improve prompt caching or stateful continuation?
- Can it parse provider output without exposing raw tool protocol in the TUI?

Do not migrate `tool_enabled` until tests prove parity with:

- create file
- overwrite file
- patch file
- shell command
- ask guidance
- malformed tool call handling
- repeated shell guard behavior
- verified action recording

### Phase 8: Internal Runtime Decision Gate

Do not build an internal model runtime in this pass.

Revisit only after Phases 1 through 7 produce measured results. An internal
runtime may eventually help because it gives Elgar direct control over:

- KV cache lifetime
- prompt reuse
- sampling and reasoning params
- model lifecycle
- GPU/Metal backend choice
- streaming/event timing

But it also makes Elgar responsible for:

- model loading
- memory pressure
- backend portability
- crashes
- model format compatibility
- tool-call parsing differences
- user configuration

It is a separate provider backend, not a replacement for the harness.

## Tests

Add focused tests before live dogfood:

- config parses request-mode profiles with defaults
- default config remains behavior-compatible with current chat-completions path
- unsupported backend/mode combinations fail clearly
- `PlainChat` still sends no tools and no `tool_choice`
- `ToolEnabled` still sends tools on the tool-capable path
- native no-tool formatting includes `reasoning` only when configured
- native no-tool formatting includes `stats` only when configured
- native parser extracts visible text without exposing reasoning as transcript
- native parser extracts stats into provider metrics
- trace/perf summary renders new stats when present and omits them cleanly when
  absent
- `hello!` remains provider-authored and no-tool
- synthesis modes remain provider-authored and no-tool

Minimum command set:

```sh
cargo fmt --check
cargo test -p elgar-core provider_config
cargo test -p elgar-core lm_studio
cargo test -p elgar-core request_modes
cargo test -p elgar-cli runtime_provider_config_loads_compatibility_metadata
./bin/check-local
```

Exact test names should follow the implementation.

## Live Dogfood

Run with the same model and same LM Studio settings before and after:

```sh
printf 'hello!\n/reasoning\n/tokens\n/exit\n' | elgar tui
elgar perf-trace
```

Then test no-tool synthesis:

```sh
printf '/permissions full_access\nRun npm run build in the current working folder and report the result. Do not edit files.\n/exit\n' | elgar tui
elgar perf-trace
```

Then test a normal tool turn:

```sh
printf '/permissions full_access\nCreate a small file named tmp-smart-provider-test.txt containing hello, then tell me what you created.\n/exit\n' | elgar tui
elgar perf-trace
```

Expected results:

- `hello!` still uses one no-tool `plain_chat` provider request
- visible text is still provider-authored
- plain chat does not inject verified folder/plan memory
- no-tool native modes report LM Studio stats when available
- tool-enabled turns continue to use the tool-capable path
- no raw tool protocol appears in the transcript
- no false claims about file creation or command execution

## Acceptance Criteria

- Default config remains backward compatible.
- Request-mode profiles are visible in traces/perf summaries.
- No-tool modes can opt into native LM Studio request fields.
- Tool-enabled mode remains on a proven custom-tool-compatible backend.
- `hello!` remains one provider request, zero tools, provider-authored.
- `hello!` latency improves materially when reasoning is lowered, or the trace
  proves the remaining bottleneck is model generation outside Elgar.
- `elgar perf-trace` shows enough provider stats to explain the result.
- Transactional shell synthesis remains a separate follow-up and is not blocked
  by this work.

## Recommended Sequencing

1. Implement Phase 1 and Phase 4 first.
   - This gives observability and behavior-preserving structure.

2. Implement Phase 2 and Phase 3 for no-tool modes.
   - This targets the `hello!` problem directly.

3. Run live dogfood and compare direct LM Studio vs Elgar.
   - Decide from evidence whether the native path is worth keeping.

4. Implement `docs/transactional-shell-synthesis-implementation-plan.md`.
   - This attacks repeated tool/model rounds after the per-call cost is clearer.

5. Consider `/v1/responses` and stateful continuation only after the first two
   tracks are measured.

## Orchestrator Handoff Summary

Ask the implementation agent to keep this planning-only until a Linear issue is
selected or created. The first implementation issue should be:

```text
LM Studio smart provider integration: request-mode profiles, native no-tool
calls, per-mode reasoning controls, and provider stats.
```

The first code pass should not touch runtime policy, tool validation, file
execution, or transactional shell synthesis. It should create the provider
profile layer and tests, then enable native no-tool LM Studio behavior behind
explicit config.
