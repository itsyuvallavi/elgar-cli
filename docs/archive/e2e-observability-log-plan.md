# Elgar E2E Observability Log Plan

## Purpose

Create one understandable end-to-end log for every Elgar turn, so we can review
what happened from user input to visible output.

The goal is not just debugging. The log should also teach the system:

```text
what file received the input
what decision happened
what request was sent
what the provider returned
what the UI rendered
what failed, if anything
```

## Current Problem

Elgar currently has two partial observability systems:

- JSONL session logs in `.elgar/log/sessions`.
- Rust `log` calls through `log` + `env_logger`.

The JSONL logs are useful, but they are not organized as a simple turn story.
The Rust logs exist, but only cover a small part of the active system.

This makes simple questions harder than they should be:

- Did this use raw chat?
- Did it attach tools?
- Did the TUI classify the input as a command?
- Did the provider use native LM Studio or OpenAI-compatible chat?
- Did the model spend time generating hidden reasoning?
- Did the UI delay rendering, or did the provider delay returning?

## Design Principle

Use one local structured log as the source of truth.

Rust `log` should be a developer console view, not the main permanent record.
Sentry should be added later for sanitized errors, not full local turn data.

The main local log record should stay local by default because it may include prompts,
model output, local file paths, timing, provider names, and future tool details.

## Target Shape

Every user turn should produce a clear sequence:

```text
turn_started
input_received
input_classified
provider_task_started
provider_request_built
provider_request_sent
provider_response_received
provider_output_parsed
session_events_recorded
ui_render_started
ui_render_finished
turn_finished
```

Later, when tools return, the same log can add:

```text
tool_call_received
tool_call_validated
policy_decision
action_started
action_finished
verification_recorded
synthesis_started
synthesis_finished
```

## Log Event Contract

Each event should include:

- `session_id`
- `turn_id`
- `event_id`
- `parent_event_id`, when useful
- `timestamp_unix_ms`
- `phase`
- `file`
- `function`
- `summary`
- `duration_ms`, when the event ends a timed step
- `metadata`

Example:

```json
{
  "session_id": "terminal-tui-session",
  "turn_id": 7,
  "event_id": "turn-7-provider-request-sent",
  "timestamp_unix_ms": 1780660323617,
  "phase": "provider",
  "file": "crates/elgar-core/src/raw_chat.rs",
  "function": "run_raw_chat_turn",
  "summary": "sent raw chat request",
  "metadata": {
    "request_mode": "raw_chat",
    "tool_count": 0,
    "message_count": 1,
    "serialized_request_bytes": 94
  }
}
```

## Privacy Rules

Default logs should avoid full raw text unless explicitly enabled.

Safe by default:

- character counts
- token counts
- request mode
- provider/backend/model
- timing
- event kinds
- tool count
- file/function ownership
- error kinds

Sensitive by default:

- full user prompt
- full model output
- full hidden reasoning
- file contents
- shell stdout/stderr
- environment variables
- API keys or URLs with secrets

Add an explicit mode later:

```text
ELGAR_LOG_DETAIL=full
```

Default should be:

```text
ELGAR_LOG_DETAIL=safe
```

## File Ownership

Create a small observability module in core:

```text
crates/elgar-core/src/observability/
  README.md
  mod.rs
  event.rs
  writer.rs
  redact.rs
```

Responsibilities:

- `event.rs`: typed event structs and phase enum.
- `writer.rs`: append JSONL events to disk.
- `redact.rs`: safe/full metadata filtering.
- `mod.rs`: small public API.
- `README.md`: beginner-friendly explanation.

Do not put provider, TUI, or policy logic inside this folder. It only records
facts that other files pass to it.

## Output Location

Use a new folder:

```text
.elgar/log/
```

File name:

```text
{session_id}.jsonl
```

Keep the existing session log while this is introduced. Do not replace it in
the first phase.

## Phase 1: Raw Chat Only

Instrument only the active raw-chat path.

Files to touch:

- `crates/elgar-tui/src/terminal/inline.rs`
- `crates/elgar-tui/src/terminal/provider_task.rs`
- `crates/elgar-core/src/raw_chat.rs`
- `crates/elgar-core/src/provider/lm_studio/native.rs`
- `crates/elgar-core/src/provider/lm_studio/openai.rs`
- `crates/elgar-core/src/session.rs`
- `crates/elgar-core/src/renderer.rs`

Expected result for plain `hello!`:

```text
input_received
input_classified: plain_text
provider_task_started
raw_chat_started
provider_backend_selected
provider_request_sent
provider_response_received
provider_output_recorded
assistant_message_recorded
ui_render_finished
turn_finished
```

Acceptance criteria:

- A plain chat turn shows exactly one provider request.
- The log clearly says `tool_count: 0`.
- The log clearly says whether backend is native or OpenAI chat completions.
- The log includes provider duration and total turn duration.
- The log includes token usage when available.
- The log does not store full prompt/output by default.

## Phase 2: TUI Rendering Details

Add enough UI timing to separate provider slowness from render slowness.

Record:

- prompt submitted
- active provider prompt shown
- live preview updated, if streaming returns later
- final assistant message rendered
- footer usage rendered

This phase should explain questions like:

```text
Did the model take 12 seconds, or did the TUI wait before rendering?
```

## Phase 3: Developer Console Logs

After the structured local log is reliable, add matching Rust `log` calls for
human console debugging.

Use:

```bash
RUST_LOG=elgar_core=debug,elgar_tui=debug,elgar_cli=debug elgar
```

The console should show short summaries only. Full details stay in JSONL.

## Phase 4: Log Viewer Command

Add a local command:

```text
/log
```

Possible variants:

```text
/log last
/log raw
/log path
```

The default `/log last` should show a simple readable summary:

```text
turn 7
input: plain text, 6 chars
provider: lm-studio native raw_chat
tools: 0
request bytes: 94
duration: 11.9s
tokens: 12 prompt, 298 completion
visible answer: 132 chars
reasoning: 870 chars, hidden/capped in UI
```

## Phase 5: Sentry Later

Sentry should receive only sanitized error or performance events.

Allowed:

- panic/error kind
- provider/backend/model
- request mode
- duration bucket
- token counts
- tool count
- file/function
- Elgar version

Not allowed by default:

- prompt text
- model text
- hidden reasoning
- file contents
- shell output
- local absolute paths, unless redacted

Sentry is useful for production-style monitoring, not for full local learning
logs.

## Premortem

Things likely to go wrong if we implement this carelessly:

- We duplicate the existing session log and create more confusion.
- We log private prompts or local file contents by accident.
- We add logging calls everywhere without a typed event contract.
- We slow down the TUI by doing blocking file writes on hot render paths.
- We make every file import a large observability API.
- We make logs hard to read by storing too much raw JSON without a summary
  viewer.
- We forget to preserve the simple raw-chat path and accidentally rebuild a
  mini harness inside logging.

Mitigations:

- Start with raw chat only.
- Keep event types small and explicit.
- Use safe metadata by default.
- Append JSONL only at important boundaries.
- Keep render-loop logging minimal.
- Add `/log last` before expanding to tools.
- Do not send anything to Sentry until local redaction is proven.

## Implementation Order

1. Add `observability/` module with typed event and local writer.
2. Add safe redaction helpers.
3. Write events for raw TUI input and command/plain classification.
4. Write events for provider task start/finish.
5. Write events for raw chat provider request/response.
6. Write events for session assistant-message recording.
7. Write events for final TUI render completion.
8. Add a tiny `/log path` command.
9. Add `/log last` summary.
10. Add Rust `log` console summaries.
11. Review privacy behavior.
12. Only then consider Sentry.

## Verification

Run:

```bash
cargo fmt
cargo check -p elgar-core
cargo check -p elgar-tui
cargo check -p elgar-cli
```

Manual check:

```bash
elgar
```

Then type:

```text
hello!
/log last
/log path
```

Expected:

- normal answer still works
- usage footer still works
- log shows raw chat
- log shows zero tools
- log shows one provider request
- no full prompt/output appears in safe mode unless explicitly enabled
