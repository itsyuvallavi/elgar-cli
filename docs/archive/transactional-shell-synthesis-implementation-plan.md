# Transactional Shell Synthesis Implementation Plan

Date: 2026-06-02
Related issue: ELG-361
Source analysis: `docs/harness-tool-loop-bottleneck-analysis.md`

## Summary

Elgar should optimize natural-language report-only shell execution by turning it
into a short verified transaction:

```text
user asks to run/report a command
-> model chooses one shell command
-> runtime validates and executor verifies it
-> runtime builds a compact verified shell digest
-> runtime stops exposing tools
-> model writes the final answer from the digest
```

This is not a new architecture. It extends patterns Elgar already has:

- explicit `/tool` shell turns can already end with `tool_result_synthesis`
- project review can already run verified inspection and then
  `project_review_synthesis`
- request modes already split `plain_chat`, `tool_enabled`,
  `tool_result_synthesis`, and `project_review_synthesis`

The missing case is natural-language `shell_execution`, for example:

```text
Run npm run build and report the result. Do not edit files.
```

That path now skips the plain route classifier, but it can still keep returning
to tool-enabled rounds after the first verified shell result.

## External Lessons

Codex, Claude Code, and Pi all preserve a similar high-level loop:

```text
model proposes tool call
harness validates/runs tool
verified result returns to model
loop continues until final answer
```

The useful lesson is not to make the harness looser. The useful lesson is to
separate raw truth from model-facing context:

- Codex appends tool observations back into the loop, manages context growth,
  and keeps prompt-cache stability in mind.
- Claude Code exposes lifecycle boundaries such as `PostToolUse` and
  `PostToolBatch`, where a harness can inspect tool results before the next
  model call.
- Pi stores structured session entries and tool results, truncates large tool
  output for model context, and keeps raw session truth separately from compact
  context.

For Elgar, the matching design is:

```text
strict validation + verified shell digest + early no-tool synthesis
```

not:

```text
less validation + let the model decide forever
```

## Problem

The UI and trace currently receive rich verified shell truth:

- command
- cwd
- exit code
- timed out flag
- elapsed time
- stdout and stderr
- truncation flags
- raw details

But model feedback often receives generic prose such as:

```text
Executed approved shell command and recorded the verified result.
```

That is true, but too weak. It does not tell the model whether the build/test
passed, what the key failure was, or whether more tools are unnecessary. With
local weaker models, this causes repeated tool rounds and wasted tokens.

Observed natural build/report result after the first fast path:

```text
provider_requests: 8
actions: 6
tool_calls: 7
provider_time_ms: 50137
tokens: 17717
```

Target:

```text
provider_requests: 2 to 3
actions: 1
tool_calls: 1
tokens: under 5000 for simple build/report
```

## First Scope

Implement only natural-language report-only shell execution.

In scope:

```text
Run npm run build and report the result. Do not edit files.
Run cargo test and tell me if it passed.
Run pnpm lint and summarize failures.
```

Out of scope for the first pass:

```text
Run npm run build, fix any errors, and keep going until it passes.
Start the dev server and keep it running.
Debug why this test is flaky.
Review this project and suggest architecture changes.
```

Those workflows need more flexible multi-step loops.

## Design

### Shell Transaction State

Add a small turn-scoped state object in core runtime:

```text
ShellTransaction
- original_user_request: String
- intent: shell_execution
- edit_allowed: bool
- report_only: bool
- primary_command_seen: bool
- primary_command_class: build | test | lint | install | dev_server | generic
- primary_result: Option<ShellActionVerification>
- result_conclusive: bool
- blocked_reason: Option<String>
```

Update this state only from validated shell tool calls and verified executor
results. Do not update it from provider prose.

### Report-Only Detection

For the first pass, keep report-only detection conservative.

Allowed signals:

- `intent.shell_execution` is already selected
- no file-editing tools are available for the route
- the user request does not ask to fix, edit, change, repair, or keep going
- the validated command is not a dev-server class command

Do not add broad natural-language trigger tables for routing. This state is
only used after the runtime has already entered `shell_execution`.

### Command Classification

Classify commands only after a validated shell command exists.

Examples:

```text
npm run build -> build
pnpm build -> build
cargo test -> test
npm test -> test
pnpm lint -> lint
npm run dev -> dev_server
```

This is command metadata, not ordinary user-text routing.

First pass only needs enough classification to avoid treating dev-server
commands as one-shot report commands. More semantic duplicate classes are
deferred.

### Conclusive Result Rules

For report-only shell execution:

```text
exit_code == 0 and timed_out == false -> conclusive success
exit_code != 0 and timed_out == false -> conclusive failure
timed_out == true -> conclusive timeout
executor error -> conclusive failure unless retry is explicitly justified
```

If the result is conclusive, close the tool phase and request synthesis.

### Verified Shell Digest

Add a core function:

```text
verified_shell_result_digest(shell: &ShellActionVerification) -> String
```

The digest should include:

- command
- cwd
- exit code
- elapsed millis
- timed out flag
- stdout summary or bounded excerpt
- stderr summary or bounded excerpt
- truncation flags
- result class: success | failure | timeout | unknown
- raw details availability
- answer_now: true when conclusive

Example success digest:

```text
VERIFIED_SHELL_RESULT
command: npm run build
cwd: /Users/yuval/__git/elgar/playground/Nextjs-1
exit_code: 0
elapsed_millis: 4800
timed_out: false
stdout_summary:
- build command completed
- no stderr
stderr_summary: empty
stdout_truncated: false
stderr_truncated: false
result_class: success
answer_now: true
raw_details_available: true
```

Example failure digest:

```text
VERIFIED_SHELL_RESULT
command: npm run build
exit_code: 1
elapsed_millis: 3200
timed_out: false
stdout_tail:
- failed to compile
stderr_summary:
- postcss.config.mjs is treated as an ES module
- module.exports is not available in ES module scope
result_class: failure
answer_now: true
raw_details_available: true
```

Keep raw stdout/stderr out of ordinary model context unless small. Use bounded
head/tail excerpts and important lines. Full raw output remains in trace,
session details, `/details last`, and `/copy raw`.

### Model Feedback

For shell actions, replace model-facing generic feedback:

```text
Executed approved shell command and recorded the verified result.
```

with:

```text
<verified shell digest>
```

Visible UI rendering can remain unchanged.

### Tool Phase Split

When a conclusive report-only shell result exists:

```text
request_mode: tool_result_synthesis
tools: 0
messages:
  system: answer using verified result only
  system: compact verified shell digest
  user: original request
```

The synthesis instruction should say:

```text
You are writing the final answer for a completed shell action.
Use only the verified result below.
Do not request or describe more tool calls.
Do not ask the user to paste output already present in the verified result.
Do not claim files were changed unless verified.
Answer briefly and directly.
```

The model still writes the final prose.

The harness may render structured verified rows, but must not hardcode final
assistant prose such as:

```text
The build passed.
```

## Expected Flow

Desired request pattern:

```text
provider request 1:
  mode: tool_enabled
  tools: shell_command + ask_guidance
  model drafts npm run build

local action:
  executor runs npm run build
  session stores raw verified result
  TUI renders compact verified result

provider request 2:
  mode: tool_result_synthesis
  tools: none
  model writes final answer from digest
```

Acceptable fallback:

```text
provider_requests: 3
tool_calls: 1 or 2
actions: 1 or 2
```

Still a failure:

```text
provider_requests near 8
multiple repeated build/probe commands
asking user to paste output Elgar already verified
```

## Implementation Steps

### Step 1: Add Core Digest Helper

Likely files:

- `crates/elgar-core/src/agent_loop.rs`
- or a new small helper module, for example
  `crates/elgar-core/src/shell_digest.rs`

Add:

```text
verified_shell_result_digest(shell: &ShellActionVerification) -> String
```

Keep it deterministic and unit-tested.

### Step 2: Feed Digest To Model

In the tool result path, when an applied action has
`VerifiedActionResult::Shell(shell)`, append the digest as the tool feedback
message instead of the generic success string.

Preserve existing UI/event/session behavior.

### Step 3: Add Shell Transaction State

Inside `run_agent_tool_chat`, track shell transaction state for
`intent.shell_execution`.

The state should know:

- whether the turn is report-only
- whether a primary shell command has completed
- whether the result is conclusive
- whether synthesis should run now

### Step 4: Request No-Tool Synthesis

When the state says the report-only shell result is conclusive:

- call the existing `tool_result_synthesis` request path
- expose zero tools
- use a small message set, not the full growing tool transcript if practical
- stop the tool loop

This should reuse the existing request mode rather than adding a new provider
mode.

### Step 5: Add Conservative Command Classification

Add only enough command classification for the first pass:

- build
- test
- lint
- install
- dev_server
- generic

Use it primarily to exclude `dev_server` from one-command terminal behavior.

Defer semantic duplicate classes until after the digest/synthesis path lands.

### Step 6: Tests

Add regression tests for:

```text
Run npm run build and report the result. Do not edit files.
```

Assertions:

- skips `plain_chat` classifier for obvious shell execution
- first provider request is `tool_enabled`
- exposed tools are `ask_guidance` and `shell_command`
- exactly one shell action is applied
- no second `shell_command` runs
- second provider request is `tool_result_synthesis`
- synthesis request has `tool_count: 0`
- final visible answer is provider-authored
- verified digest is present in model-facing context
- raw stdout/stderr remain available through raw details

Also keep existing guardrail tests:

```text
What does cargo test do?
```

Expected:

- `plain_chat`
- tools exposed: 0

### Step 7: Live Dogfood

Run:

```text
/permissions full_access
Run npm run build and report the result. Do not edit files.
/exit
```

Expected:

```text
provider_requests: 2 or 3
actions: 1
tool_calls: 1
second shell commands: 0
tokens: under 5000 for simple build/report
visible answer: model-authored pass/fail summary grounded in verified result
```

Also run:

```text
What does cargo test do?
```

Expected:

```text
plain_chat
tools_exposed: 0
```

## What Not To Do

Do not hardcode final assistant replies.

Do not cap assistant output globally as the core latency fix.

Do not add broad natural-language trigger tables.

Do not move raw stdout into visible chat by default.

Do not weaken runtime validation, policy, or executor verification.

Do not apply one-command terminal behavior to dev-server/debug/fix loops.

## Deferred Work

Defer until after the first pass:

- semantic duplicate shell classes for build/test/lint variants
- broader debug/fix loop budgets
- dev-server background-process contract
- token-based shell digest tuning
- provider-mode-specific model selection
- route-specific reasoning/effort controls

## Acceptance Criteria

- Natural report-only shell execution uses 2 to 3 provider requests.
- It applies exactly one primary shell action for simple build/test/lint report
  prompts.
- It does not run redundant `ls .next`, repeated build, temp-output, or probe
  commands after a conclusive result.
- The final answer is model-authored.
- The final answer is grounded in the verified shell digest.
- The model does not ask the user to paste output already verified by Elgar.
- Raw details remain available through existing raw/details paths.
- Plain chat remains cheap and tool-free.
- Command explanation questions remain plain chat.
- Runtime validation, policy, and executor truth are unchanged.

## Verification Commands

Run at minimum:

```sh
cargo fmt --check
cargo test -p elgar-core shell_execution
cargo test -p elgar-core request_modes_split_tool_and_tool_result_synthesis_without_caps
cargo test -p elgar-core command_question_stays_plain_chat_first
cargo test -p elgar-core --lib
./bin/check-local
```

Then run installed live dogfood:

```sh
printf '%s\n' '/permissions full_access' 'Run npm run build and report the result. Do not edit files.' '/exit' | elgar tui
elgar perf-trace
```

The perf trace should show:

```text
tool_enabled
tool_result_synthesis
tools on synthesis: 0
one shell action
no redundant shell probes
```
