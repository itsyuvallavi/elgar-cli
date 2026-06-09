# Native Tool Loop

## Purpose

Define the target harness loop Elgar should move toward.

This document exists because the harness is moving from a text/JSON
model-choice loop toward native provider tool calls.

- native provider tool calls
- text/JSON model-choice decisions plus separate synthesis

The target is the standard agent loop used by Codex/Pi/Claude-style tools:

```text
user prompt
-> model call with native tool schemas
-> model returns tool calls or final text
-> runtime validates and executes tool calls
-> runtime appends assistant tool-call message + tool result message
-> model continues with more tool calls or final text
```

## External Reference

LM Studio's OpenAI-compatible tool-use docs describe the same high-level flow:

- provide tool definitions in `/v1/chat/completions`
- inspect `choices[0].message.tool_calls`
- execute those functions locally
- append both the assistant tool-call message and the `role:"tool"` result
  message back to `messages`
- call the model again for a normal response or more tool use

Source: <https://lmstudio.ai/docs/developer/openai-compat/tools>

## Current Elgar Loop

Current active path:

```text
user prompt
-> harness native tool loop call
-> provider gets native tool schemas
-> provider returns native tool_calls, JSON/text fallback, or final text
-> native tool_calls are validated directly
-> JSON/text fallback is converted into ModelChoice only when needed
-> Rust validates and executes read-only primitives
-> Elgar appends assistant tool-call messages plus role:"tool" result messages
-> final normal text ends the loop
```

Current strengths:

- one harness route from CLI/TUI to provider
- no raw-chat bypass
- primitive read-only tools only: `read`, `ls`, `find`, `grep`
- Rust validates tool names and arguments
- Rust collectors produce verified evidence
- provider tool calls are already preferred when present
- JSON parsing exists as fallback only
- logs show rounds, provider calls, tools, tokens, repairs, and stop reason

Current weaknesses:

- `answer_now` still exists as a legacy JSON fallback shape
- synthesis still exists as a fallback path
- logs should more clearly report final source: native final text vs fallback
  synthesis
- live LM Studio quality and token behavior still need validation

## Target Loop

Target active path:

```text
user prompt
-> model call with native provider tool schemas
-> if provider returns tool_calls:
     Rust validates requests
     Rust executes tools
     Rust appends:
       assistant message with original tool_calls
       tool result message for each tool_call_id
     loop
-> if provider returns normal text:
     visible final answer
```

Rules:

- no macro tools
- no `review_project`
- no hardcoded natural-language trigger table
- no `answer_now` control tool unless later proven necessary
- JSON text parsing is fallback only
- final text is the normal loop terminator
- synthesis is fallback/optional, not the default end state
- runtime still owns validation, permissions, execution, and truth

## Harness Folder Audit

### `model_choice/`

Current role:

- renders the legacy/fallback model-choice contract
- parses JSON text fallback
- validates structured request JSON
- contains prose guards and provider marker cleanup

Target role:

- fallback parser only; this is now the intended direction
- provider-native `tool_calls` should not depend on this folder
- keep JSON support for models/providers that fail native tool calls
- keep prose guards for unsafe mixed text + tool-shaped fallback output

### `tool_definitions.rs`

Current role:

- converts executable primitive registry entries into OpenAI-compatible tool
  schemas
- currently covers `read`, `ls`, `find`, and `grep`

Target role:

- primary model-visible tool surface
- schemas should carry as much protocol structure as possible
- primitive registry remains the source of truth for what exists and what can
  execute now

### `harness_loop/provider/`

Current role:

- builds decision/repair prompts
- calls provider with tools for decisions
- calls provider without tools for synthesis
- builds compact evidence summaries into prompt text

Target role:

- own native model calls over a growing `Vec<ChatMessage>`
- attach tool schemas on tool-capable turns
- preserve assistant tool-call messages
- append `role:"tool"` messages with verified tool results
- stop when the model returns normal text
- keep repair and synthesis as fallback paths only

### `harness_loop/evidence/`

Current role:

- executes validated read-only primitive requests
- renders evidence bodies for model-facing summaries and synthesis
- tracks labels, bytes, truncation, and duplicate keys

Target role:

- execute validated primitives
- return a bounded tool-result payload suitable for `ChatMessage::tool`
- retain exact/full verified evidence locally for logs/details
- keep compact summaries only for UI/logs/fallback, not as the primary model
  protocol

### `harness_loop/provider/synthesis.rs`

Current role:

- asks the model for a final answer without tools
- receives full verified evidence as prompt text
- is the fallback final path after `answer_now`, duplicate-loop stop, or other
  explicit safe-stop reasons

Target role:

- optional fallback only
- use when the tool loop cannot continue or when we intentionally need a no-tool
  summarizer
- not the default successful ending for normal native tool loops

## Provider Support Finding

Elgar already has the provider vocabulary needed for native tool-result loops:

- `ChatMessage` supports `role: Tool`
- `ChatMessage::tool(tool_call_id, content)` exists
- `ChatMessage` can serialize `tool_call_id`
- `ChatMessage` can carry assistant `tool_calls`
- LM Studio OpenAI-compatible request formatting serializes `messages`
- LM Studio response parsing already reads `message.tool_calls`

Implemented active harness behavior:

- after executing a tool call, the loop appends the original assistant
  tool-call message and matching `role:"tool"` result message back into the
  provider conversation
- normal final text ends the loop
- JSON fallback uses synthetic tool-call messages so later calls still see a
  native conversation shape

Remaining active harness gaps:

- logs should label decision source and final source more explicitly
- `answer_now` should be removed or downgraded further once live results prove
  it is no longer needed
- synthesis should remain fallback-only and be measured separately

## Transition Plan

### Slice 1: Provider Tool-Result Harness Prototype

Goal:

Build a native tool-message loop path behind tests without changing TUI behavior
yet.

Likely files:

- `crates/elgar-core/src/harness/harness_loop/provider/native_loop.rs`
- `crates/elgar-core/src/harness/harness_loop/provider/mod.rs`
- `crates/elgar-core/src/harness/harness_loop/evidence/execution.rs`
- `crates/elgar-core/src/harness/harness_loop/control/choice_from_output.rs`
- `crates/elgar-core/src/harness/tests/loop_flow/`

Acceptance:

- native provider `tool_calls` are executed
- assistant tool-call message is preserved
- matching `role:"tool"` result message is appended
- a following provider call can return final text
- JSON fallback remains unchanged

Status: implemented in core tests.

### Slice 2: Make Native Loop Primary For Read-Only Harness

Goal:

Use the native tool-message loop for `read`, `ls`, `find`, and `grep` in the
normal CLI/TUI path.

Acceptance:

- `model_choice/` is used only when no native `tool_calls` exist
- final normal text ends the loop
- no successful read-only turn requires `answer_now`
- synthesis is not used for normal successful native loops

Status: implemented for core loop behavior and validated through live CLI
read-only prompts.

### Slice 3: Reclassify Synthesis

Goal:

Make synthesis an explicit fallback path.

Acceptance:

- docs and logs distinguish `final_text` from `fallback_synthesis`
- `elgar logs latest` reports whether final text came directly from native loop
  or from synthesis fallback
- post-evidence malformed control-looking text is treated as final text unless
  it is valid fallback control JSON

### Slice 4: Remove Or Downgrade Text-Only Control Paths

Goal:

Avoid custom control protocol becoming the main route again.

Acceptance:

- `answer_now` is removed or marked legacy/fallback if no longer needed
- JSON structured request parsing remains only as fallback
- tests prove native tool calls are preferred

## Pre-Mortem And Mitigation

Risk: LM Studio accepts tool result messages but the loaded model handles them
poorly.

Mitigation:

- add one live probe before switching default behavior
- keep current synthesis loop behind a fallback flag/path until live results
  are acceptable

Risk: preserving assistant tool-call messages increases prompt size.

Mitigation:

- tool result payloads must be bounded
- exact evidence remains local; model-facing tool result can be compact
- compare logs before and after switching

Risk: final text after tool results becomes less structured than synthesis.

Mitigation:

- keep a concise system prompt for final response style
- do not remove synthesis until native final answers are good enough

Risk: JSON fallback and native path overlap in confusing ways.

Mitigation:

- provider `tool_calls` always win
- fallback parser runs only when `tool_calls` is empty
- logs record `decision_source: native_tool_calls | json_fallback | text`

Risk: permissions become harder later.

Mitigation:

- native loop still routes every tool call through Rust validation and future
  policy before execution
- do not enable `bash`, `write`, or `edit` in this transition

Risk: TUI rendering regresses.

Mitigation:

- keep visible assistant event shape stable
- add focused tests before live TUI checks
- use logs to verify final text source

## Testing Plan

Focused tests:

```text
cargo test -p elgar-core harness::tests::loop_flow -- --nocapture
cargo test -p elgar-core harness::tests::model_choice -- --nocapture
cargo check -p elgar-cli
```

Live prompts after implementation:

```text
elgar "hello"
elgar "read package.json"
elgar "list app"
elgar "read app"
elgar "grep for tailwind"
elgar "review the project"
elgar logs latest
```

Measure:

- provider calls
- prompt tokens
- completion tokens
- repairs
- final source
- tools used
- stop reason
- answer quality
