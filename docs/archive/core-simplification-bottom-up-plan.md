# Elgar Core Simplification Bottom-Up Plan

## Purpose

Simplify Elgar by starting from the smallest useful runtime:

```text
user prompt -> provider request -> model answer -> visible response
```

Then add capabilities back one layer at a time, with clear ownership, tests, and
review gates.

This plan is intentionally conservative. The goal is not to delete or move many
files quickly. The goal is to make every file explainable, prove what is still
needed, and prevent the harness from quietly deciding user intent.

## Rebuild Vision

Raw chat is the baseline, not the final architecture.

The rebuild should simplify Elgar without forcing a full rewrite when features
return. The permanent skeleton should stay small and stable:

```text
provider request/response
session/events
visible message rendering
CLI/TUI input loop
provider/model config
logs/traces
```

Everything else is an optional capability that must plug into that skeleton
through a narrow contract. Tools, permissions, shell execution, memory, plans,
state answers, synthesis, and project review should not be mixed into the raw
chat path.

Guiding rule:

```text
Archive legacy feature implementations.
Keep the reusable skeleton.
Re-add features one at a time through explicit module boundaries.
```

This avoids two bad outcomes:

- keeping the current tangled harness, where hidden behavior leaks into simple
  chat
- deleting too much, then having to rewrite the whole project when features are
  added back

## Permanent Skeleton Versus Optional Features

The permanent skeleton is allowed to know how to:

- send a provider request
- receive provider output
- record events
- expose visible provider-authored text
- render conversation state
- load provider configuration
- write basic logs/traces

The permanent skeleton must not decide natural-language intent.

Optional features must be added as separate layers:

```text
tools/
permissions/
memory/
planning/
shell/
synthesis/
observability/
```

Each optional layer needs:

- one clear entry point
- typed input/output
- tests at its boundary
- no hardcoded ordinary-language trigger table
- no harness-authored assistant prose

The model should decide what the user is asking for. The harness should
validate, enforce policy, execute approved actions, verify results, and report
structured facts.

## Re-Add Sequence

After raw chat is stable, features should come back in this order:

1. Structured logging and trace visibility.
2. Tool-call parsing only, with no execution.
3. Permission gate.
4. One read-only inspection tool.
5. Write tools.
6. Shell execution.
7. Bounded memory.
8. Planning.
9. Synthesis and project review.

Do not skip ahead because that recreates the current problem: multiple systems
interacting before their boundaries are understandable.

## Current Verified Baseline

Verified on 2026-06-04 from `/Users/yuval/__git/elgar`.

```text
cargo check -p elgar-core
cargo check -p elgar-tui
cargo check -p elgar-cli
```

Result:

```text
PASS
```

Core source inventory:

```text
crates/elgar-core/src files: 73
top-level .rs files: 56
```

Large files currently visible:

```text
crates/elgar-core/src/agent_loop.rs          987 lines
crates/elgar-core/src/agent_runtime.rs       820 lines
crates/elgar-core/src/session.rs            1903 lines
crates/elgar-core/src/provider/lm_studio.rs 1651 lines
```

Full lib test baseline:

```text
cargo test -p elgar-core --lib -- --test-threads=1
```

Result:

```text
FAIL: 411 passed, 23 failed
```

All current failures are in `agent_loop::tests`. This means future work must not
claim a clean full suite until those failures are fixed or intentionally retired.

## Current Problem

The project has too many overlapping runtime paths:

- `agent_runtime.rs` owns the newer runtime entry point.
- `agent_loop.rs` still owns a large part of the model/tool turn behavior.
- `controller.rs` and controller helper files still exist as compatibility or
  older routing surfaces.
- provider, session, policy, actions, memory, plans, shell, and visibility all
  interact inside the same runtime flow.
- some labels can be misleading. A path named like plain chat may still involve
  classification, prompt injection, memory, or follow-up synthesis.

The practical result is that a simple request can accidentally pass through
extra machinery:

```text
user text
-> route decision
-> memory/context injection
-> tool eligibility
-> policy checks
-> tool result rendering
-> synthesis request
-> visible answer filtering
```

For the user's target behavior, that is too much.

## Target Mental Model

The simplified system should have explicit layers:

```text
UI
Runtime
Provider
Session/events
Optional capabilities
```

Plain user text should be model-first:

```text
TUI/CLI input
-> raw chat runtime
-> provider
-> provider-authored assistant response
-> UI renders it
```

Optional features should be added only when explicitly enabled:

```text
slash commands
tools
permissions
shell/filesystem execution
memory
plans
state answers
project review
```

No ordinary phrase like "review the project" should cause hidden harness
behavior unless the model asks for a typed tool or the user uses an explicit
local command.

## Smallest Useful System

The minimum useful Elgar core should contain:

```text
provider types
provider config
provider HTTP or stub
session/events
raw chat function
thin CLI/TUI command path
```

It should not contain:

```text
tool calls
permissions
plans
memory
project review behavior
route classifier
state-answer fallback
shell execution
filesystem actions
```

Planned new file:

```text
crates/elgar-core/src/raw_chat.rs
```

Expected responsibility:

```text
Take user text and provider config.
Send exactly one plain provider request.
Record provider start/finish events.
Record one visible provider-authored assistant message.
Return without tools, memory, route classification, or synthesis.
```

## Proposed Future File Categories

This is the target organization. It should be introduced gradually.

```text
crates/elgar-core/src/
  raw_chat.rs                  smallest runtime path
  runtime/                     normal agent runtime and tool loop
  provider/                    provider config, HTTP, LM Studio parsing
  state/                       session, events, logs, traces
  actions/                     typed actions, policy decisions, approval gate
  tools/                       tool validation, tool output, anchors, scope
  shell/                       shell executor and allowlist
  plans/                       plan contract and plan tree
  memory/                      session and artifact memory
  ui_contract/                 data shapes the CLI/TUI can render
  compat/                      legacy controller paths
  tests/                       shared test helpers and integration-style tests
```

Important: do not move everything into folders immediately. First create the
inventory and raw path. Then move one category at a time.

## Phase 0: Inventory Before More Code

Goal:

Create a complete source map where every file in `crates/elgar-core/src` has an
owner and a reason to exist.

Deliverables:

- Update `docs/elgar-core-file-map.json` so every file is listed individually.
- Update `docs/elgar-core-file-map.html` so the map has no vague wildcard
  groups.
- Add these fields for each file:

```text
file path
current line count
current category
public module or internal module
imported by
imports
needed for raw chat
needed for TUI
needed for full agent
safe to move
safe to delete
notes
```

Verification:

```text
cargo check -p elgar-core
git diff --check
```

Premortem:

- If the inventory is wrong, we may delete or move a file that is still needed.
- If imports are guessed manually, hidden test-only dependencies will be missed.
- If wildcard groups remain, the map will keep hiding complexity.

Mitigation:

- Generate import data from source, then manually review it.
- Mark unknown files as `unknown`, not `unused`.
- Do not delete files in this phase.
- User reviews the inventory before any structural move.

Review gate:

Do not continue until the user reviews the full file inventory.

## Phase 1: Add Raw Chat Core

Goal:

Create the bottom-most runtime path that sends a prompt to the model and returns
the model response.

Expected file:

```text
crates/elgar-core/src/raw_chat.rs
```

Possible public API:

```text
pub fn run_raw_chat_turn(...)
```

The exact signature should be chosen from existing provider/session types after
reviewing the current provider trait.

Rules:

- one user message in
- one provider request out
- no route classifier
- no tools
- no `tool_choice`
- no memory/context injection
- no shell/filesystem action
- no permission check
- no project review handling
- no second synthesis request
- visible assistant text must come from the provider

Verification:

```text
cargo check -p elgar-core
cargo test -p elgar-core raw_chat -- --test-threads=1
```

Add focused tests proving:

- raw chat sends exactly one provider request
- raw chat attaches zero tools
- raw chat records provider-authored assistant text
- raw chat does not import or call `agent_loop`
- empty provider output becomes a clear provider/runtime error, not fake prose

Premortem:

- The provider trait may be coupled to agent-loop request metadata.
- Session logging may require event shapes that assume normal agent turns.
- The TUI may expect existing `AgentRuntime` output instead of raw chat output.
- A stub provider test may pass while LM Studio formatting still sends extra
  fields.

Mitigation:

- Test with the stub provider and inspect serialized provider payloads.
- Keep raw chat in core, not in CLI/TUI.
- Do not remove the existing agent path yet.

Review gate:

User reviews the raw chat file and tests before wiring it into the UI.

## Phase 2: Add Explicit TUI Raw Mode

Goal:

Let the user run the raw path from the TUI without changing normal chat yet.

Preferred trigger:

```text
/raw <prompt>
```

Reason:

`/raw` is explicit. It does not require a hardcoded natural-language trigger
table.

Rules:

- `/raw hello` uses `raw_chat.rs`.
- normal `hello` keeps using the current runtime until the raw path is proven.
- the UI only chooses the explicit command path; it does not decide normal user
  intent.

Verification:

```text
cargo check -p elgar-core
cargo check -p elgar-cli
cargo check -p elgar-tui
```

Smoke test target:

```text
/raw hello
```

Expected behavior:

```text
one provider request
zero tool calls
one visible provider answer
```

Premortem:

- The TUI command parser may already use `/raw` or route slash commands through
  another layer.
- Rendering may hide provider-authored messages if source labels differ.
- The command may accidentally call `AgentRuntime` instead of raw chat.

Mitigation:

- Add a boundary test at the CLI/TUI command layer if practical.
- Trace request count and request mode during the smoke test.
- Keep normal chat unchanged during this phase.

Review gate:

User reviews the explicit `/raw` behavior before it becomes the default path.

## Phase 3: Decide Whether Raw Chat Becomes Default Plain Chat

Goal:

Choose whether normal text should use raw chat first.

Decision options:

```text
Option A: normal text -> raw_chat first
Option B: normal text -> AgentRuntime plain-provider path
Option C: keep /raw only while existing plain chat is repaired
```

Recommended starting choice:

```text
Option C
```

Reason:

The full lib suite is already red in `agent_loop` behavior. Making raw chat the
default immediately could hide those failures instead of clarifying them.

Verification for any default-path change:

```text
cargo check -p elgar-core
cargo test -p elgar-core normal_text_model_plain_answer_renders_without_tools -- --test-threads=1
cargo test -p elgar-core trivial_greeting_uses_plain_provider_request -- --test-threads=1
```

Premortem:

- If raw chat becomes default too early, tool-capable requests may stop working.
- If the switch is based on ordinary phrases, hardcoded routing comes back.
- If stateful follow-up behavior is bypassed, normal project work may lose
  context unexpectedly.

Mitigation:

- Keep the default switch behind a clear config flag or explicit mode first.
- Define what counts as raw chat versus agent chat before changing normal text.
- Add tests at provider payload boundary.

Review gate:

User approves the default-path decision explicitly.

## Phase 4: Categorize Files Without Moving Them

Goal:

Make every current file explainable before structural moves.

Categories:

```text
raw chat
provider
session/state
normal agent runtime
tool handling
policy/actions
shell/filesystem
plans
memory/context
visibility/rendering
controller compatibility
tests/helpers
docs
unknown
```

Verification:

```text
cargo check -p elgar-core
```

Premortem:

- A file may look unused but exist for tests, public API compatibility, or
  provider-specific behavior.
- Moving files before category review will make the project harder to reason
  about.
- Controller compatibility may be ugly but still required by CLI/TUI tests.

Mitigation:

- Use categories in the HTML/JSON map first.
- Do not move files in this phase.
- Mark unclear files as `unknown` and inspect them later.

Review gate:

User reviews categories and chooses the first folder group to move.

## Phase 5: Move One Folder Group At A Time

Goal:

Reduce source root clutter without creating new huge files.

Rules:

- Move one category at a time.
- Run verification after each category.
- Keep moved files roughly the same size or smaller.
- Do not combine unrelated files.
- Do not move the huge test file again without explicit approval.

Candidate move order:

```text
1. tiny provider-event helpers
2. tool helper files
3. policy/action helper files
4. memory/context files
5. plan files
6. shell files
7. controller compatibility files
8. session/state files
9. agent runtime files
```

Reason for this order:

Start with smaller, lower-risk groups. Leave `session.rs`, `provider/lm_studio.rs`,
and core runtime files until the import graph is clearer.

Verification after each group:

```text
cargo fmt
cargo check -p elgar-core
cargo test -p elgar-core --lib <focused_test_name> -- --test-threads=1
git diff --check
```

Premortem:

- Rust module paths may break across public/private boundaries.
- Tests using `super::*` may fail after moves.
- Moving compatibility files may break CLI or TUI crates, not only core.
- A folder may become another dumping ground if responsibilities are not narrow.

Mitigation:

- Move only one group per branch/step.
- Keep old module names re-exported temporarily when useful.
- Prefer small `mod.rs` or re-export shims over broad API rewrites.
- Record line counts before and after every move.

Review gate:

User approves each move group before it happens.

## Phase 6: Fix Or Retire Current Failing Agent Loop Tests

Goal:

Turn the current red suite into a useful safety net.

Current status:

```text
411 passed
23 failed
```

Failure themes:

- plain chat request count changed from 1 to 2
- provider-authored assistant message not recorded where expected
- plan creation/execution paths do not create expected files
- state-answer and runtime-block behavior changed
- preflight and path anchoring expectations changed

Approach:

1. Group failures by behavior, not by test order.
2. Decide whether each behavior is still desired under v0.10.
3. Fix tests that describe desired behavior.
4. Retire or rewrite tests that describe old controller/harness behavior.

Verification:

```text
cargo test -p elgar-core --lib -- --test-threads=1
```

Premortem:

- Some failing tests may encode old behavior that should not come back.
- Fixing all failures mechanically may reintroduce hardcoded harness behavior.
- Ignoring the failures will make future refactors impossible to trust.

Mitigation:

- For each failing test, write one sentence: keep, rewrite, or delete.
- User reviews that classification before changes.
- Do not chase green tests by restoring old phrase routing.

Review gate:

User approves the failing-test classification before fixes.

## Phase 7: Add Capabilities Back Deliberately

Goal:

Build up from raw chat to full agent behavior with visible ownership.

Recommended order:

```text
1. raw provider chat
2. explicit slash commands
3. session logs/traces
4. typed tools
5. policy and permissions
6. filesystem executor
7. shell executor
8. bounded memory/context
9. plans
10. state answers
11. full agent loop
```

For every capability, document:

```text
why it exists
which files own it
how the model requests it
how the runtime validates it
how policy approves or blocks it
how the executor verifies it
how the UI displays it
which tests protect it
```

Premortem:

- Adding memory too early may recreate huge hidden prompts.
- Adding tools too early may recreate "data but no answer" behavior.
- Adding plans too early may make every build request enter plan machinery.
- Adding state answers too early may make the harness speak instead of the
  model.

Mitigation:

- Keep every capability opt-in until tested.
- Prefer explicit slash commands for local control.
- Let the model own intent for normal language.
- Keep provider-authored answers visible unless there is a typed verified fact
  to render.

## Do Not Do

Do not:

- archive or delete all source files at once
- move the giant test file again without explicit user approval
- replace model answers with hardcoded harness prose
- add natural-language trigger tables
- make TUI own core runtime behavior
- turn the HTML map into a substitute for tests
- call existing red tests "new failures" without checking the baseline
- move `session.rs` or `provider/lm_studio.rs` early just because they are large
- hide old behavior behind new names like `plain_chat`

## Verification Matrix

Use these commands depending on the step.

Compile core:

```text
cargo check -p elgar-core
```

Compile TUI after UI wiring:

```text
cargo check -p elgar-tui
```

Compile CLI after command wiring:

```text
cargo check -p elgar-cli
```

Focused raw chat tests:

```text
cargo test -p elgar-core raw_chat -- --test-threads=1
```

Full core test baseline:

```text
cargo test -p elgar-core --lib -- --test-threads=1
```

Formatting:

```text
cargo fmt
```

Patch hygiene:

```text
git diff --check
```

Runtime smoke tests should capture:

```text
prompt
provider request count
tools attached or not
assistant message source
visible response text
events emitted
```

## Pessimistic Failure Predictions

| Change | What may break | Why it may break | Mitigation |
| --- | --- | --- | --- |
| Add `raw_chat.rs` | provider tests pass but live LM Studio still gets extra fields | request formatting may be shared with agent mode | inspect serialized payloads |
| Add `/raw` TUI command | TUI renders nothing | UI may filter unfamiliar event source | reuse provider assistant source |
| Make raw chat default | tool requests stop working | normal text may bypass AgentRuntime | keep `/raw` explicit first |
| Move tool files | imports break | modules are currently flat | move one group, compile immediately |
| Move session/state | many crates break | `session.rs` is central and large | delay until inventory is reviewed |
| Fix failing tests | old harness behavior returns | tests may encode bad behavior | classify tests before fixing |
| Delete "unused" files | hidden compatibility breaks | public modules and tests may depend on them | no deletion before import graph |
| Add memory back | prompt grows again | memory can become implicit context injection | bound memory and test plain chat |
| Add plan flow back | simple requests become plan machinery | intent boundary is unclear | model-owned typed intent only |
| Add state answers back | harness speaks instead of model | verified state rendering can look like assistant prose | clearly label verified state |

## Review Gates

Gate A:

```text
full file inventory reviewed
```

Gate B:

```text
raw_chat.rs API and tests reviewed
```

Gate C:

```text
/raw TUI behavior reviewed
```

Gate D:

```text
file category map reviewed
```

Gate E:

```text
first folder move approved
```

Gate F:

```text
failing agent_loop test classification approved
```

No broad refactor should happen before the matching gate is approved.

## Recommended First Implementation Step

Start with Phase 0 only.

Concrete next task:

```text
Update docs/elgar-core-file-map.json and docs/elgar-core-file-map.html so every
file under crates/elgar-core/src is shown individually with category, purpose,
imports, imported-by, and raw-chat relevance.
```

Why this first:

It gives the user a complete map before code moves. It also prevents accidental
deletion of files that are ugly but still required.

After that, implement `raw_chat.rs` as a narrow core path and keep it separate
from the current agent runtime until it is proven.
