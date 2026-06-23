# Elgar Product Architecture Plan

Date: 2026-05-25

Status snapshot: 2026-05-31

Legend:

- Complete: implemented and verified enough to treat as current baseline.
- Partial: implemented in the main path, but cleanup, edge cases, or coverage
  remain.
- Pending: intentionally not started or still design-only.

## Product Direction

Elgar should become:

```text
Pi-like terminal UX
+ Codex-like coding capability
+ Elgar-owned verified trust
```

This means the user should feel like they are talking to a capable coding
agent, not a router. The model should understand the request, reason about
context, choose tools, and ask for guidance when uncertain. The runtime should
own permission, execution, verification, and truthful reporting.

## Operating Contract

The new contract is:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors apply.
Filesystem/shell verify.
UI reports.
Tests protect.
```

This replaces the old normal-chat shape where controller routing tried to infer
intent before the model. The controller can remain for legacy explicit smoke
paths and compatibility tests, but it must not be the conversational brain for
normal CLI/TUI chat.

## Rebuild Guideline

Elgar is being rebuilt from the smallest working baseline:

```text
user prompt -> provider request -> model answer -> visible response
```

That baseline is not the final product. It is the stable foundation.

The permanent skeleton should stay small:

- provider request/response
- session/events
- visible message rendering
- CLI/TUI input loop
- provider/model configuration
- basic logs/traces

Capabilities should be re-added as separate modules with explicit contracts:

- tools
- permissions
- memory
- planning
- shell execution
- synthesis
- observability

The harness must not infer ordinary-language intent. The model decides intent.
The harness validates typed model requests, enforces policy, executes approved
actions, verifies results, and reports structured facts.

This lets Elgar simplify now without requiring a full rewrite later. Legacy
feature implementations may be archived as reference, but future features must
plug into the stable skeleton instead of recreating the old all-in-one harness.

## Plain Chat Provider Boundary

Plain/simple user messages must get a plain provider request before any
tool-capable workflow is attempted.

For inputs like `hello`, `say hi`, `what are you?`, or `write a short
sentence`, the first provider request must:

- omit tools
- omit `tool_choice`
- omit latest-folder and project-plan memory
- avoid folder anchoring
- avoid workflow phrase handling
- use the same documented provider, model, and stream configuration source as
  normal runtime

Tool-capable turns are still part of the target product, but they must be gated
by explicit user intent or explicit runtime state. Examples include direct file
creation/editing requests, project inspection/modification requests, or an
active confirmed plan state that requires file operations. Do not add new
hardcoded phrase lists, model names, provider-specific hacks, or prompt-only
routing to satisfy this boundary.

The provider payload is the testable contract. Runtime and provider tests must
assert the serialized request shape for both plain chat and tool-needed chat.

## Project Structure From Now On

Keep the project organized by ownership boundaries, not by historical
implementation order.

### Core Runtime

Status: Partial.

Current state:

- AgentRuntime is the normal runtime path for CLI/TUI conversation work.
- Plain chat, plan creation, plan execution, verified state answers, artifact
  memory, and runtime-block reporting are substantially improved.
- `agent_loop.rs` still owns too many responsibilities and remains the highest
  risk file to split later.
- Intent-scoped tool exposure and broader route/performance tuning remain open.

Files/modules:

- `crates/elgar-core/src/agent_runtime.rs`
- `crates/elgar-core/src/agent_loop.rs`
- `crates/elgar-core/src/model_runtime.rs`
- `crates/elgar-core/src/policy.rs`

Responsibilities:

- Build model prompt/context.
- Send user turns to the provider.
- Validate tool calls.
- Convert tool calls into typed actions.
- Apply permission policy.
- Record policy decisions and action lifecycle events.
- Never render UI.
- Never silently bypass policy.
- Keep plain chat plain on the first provider request.

Near-term cleanup:

- Keep reducing direct `agent_loop.rs` responsibility once regression coverage
  is stronger.
- Keep runtime/tool validation structural; do not add natural-language trigger
  tables.
- Finish intent-scoped tool exposure so shell-only turns cannot choose file
  creation tools before validation.

### Executors

Status: Partial.

Current state:

- Filesystem writes are applied through verified action results.
- Plan execution verifies expected files/folders before reporting completion.
- Shell commands exist but are deliberately not the current focus until file
  planning/creation remains stable under live TUI dogfood.

Files/modules:

- `crates/elgar-core/src/fs.rs`
- `crates/elgar-core/src/shell.rs`
- future small helpers under `crates/elgar-core/src/executors/` only if needed

Responsibilities:

- Apply already-approved or policy-approved actions.
- Enforce allowed roots and path safety.
- Verify filesystem/shell results.
- Return structured success or failure.
- Never decide conversational intent.
- Never render UI.

### Action And Approval Layer

Status: Partial.

Current state:

- Policy-approved filesystem actions are recorded and reported.
- Explicit `/approve` and `/reject` remain local UI actions.
- Review/pending flows still need continued transcript and grouping polish.

Files/modules:

- `crates/elgar-core/src/action.rs`
- `crates/elgar-core/src/action_gate.rs`
- `crates/elgar-core/src/policy.rs`

Responsibilities:

- Represent action lifecycle as typed data.
- Route explicit `/approve` and `/reject`.
- Distinguish user approval from policy approval.
- Keep rejected/failed/applied states terminal and auditable.

### Provider Layer

Status: Partial.

Current state:

- LM Studio/OpenAI-compatible local provider flow works for current dogfood.
- Provider request/token metadata feeds `/tokens`, footer display, and traces.
- Context-window display now uses configured/provider-backed values instead of
  pretending estimated local context is provider usage.
- LM Studio-specific context discovery/config sync is deferred.

Files/modules:

- `crates/elgar-core/src/provider/`
- `crates/elgar-core/src/provider_visible.rs`

Responsibilities:

- Speak LM Studio/OpenAI-compatible APIs.
- Parse provider text, reasoning, tool calls, and errors.
- Keep provider quirks isolated from runtime policy and TUI rendering.
- Keep deterministic stub behavior useful for tests, but do not let it define
  product behavior.

### Memory And Context

Status: Partial.

Current state:

- Verified folders, plans, structured plans, and ordinary verified artifacts are
  available to follow-up tool/state turns.
- State answers can report created files, plan status, first/latest artifacts,
  and project-scoped artifact lists more reliably.
- Plain chat remains isolated from project memory.
- Memory is still session-local; durable multi-session memory and compaction are
  pending.

Files/modules:

- `crates/elgar-core/src/context.rs`
- `crates/elgar-core/src/controller_project_memory.rs`
- `docs/read-only-memory-context.md`
- future `USER.md` / `MEMORY.md` work

Responsibilities:

- Provide small, verified, bounded context to the model.
- Prefer verified filesystem/session facts over transcript guessing.
- Keep memory read-only during a turn unless an explicit memory feature is
  being implemented.
- Avoid turning memory into a transcript dump.

Near-term cleanup:

- Rename or split controller-named memory modules once the AgentRuntime path is
  stable, so names match ownership.

### TUI And CLI

Status: Partial.

Current state:

- CLI and TUI run through the AgentRuntime path for normal chat.
- TUI cursor movement, footer layout, working state, and plan display have been
  improved.
- CLI/TUI modules have been split into smaller files for most non-core code.
- More transcript polish remains, especially model/tool progress summaries and
  plan/action grouping.

Files/modules:

- `crates/elgar-cli/src/`
- `crates/elgar-tui/src/terminal.rs`
- `crates/elgar-tui/src/terminal/provider_task.rs`
- `crates/elgar-tui/src/panes.rs`
- `crates/elgar-tui/src/action_panel.rs`

Responsibilities:

- Submit user text to `AgentRuntime`.
- Keep slash commands local and explicit.
- Render model text, tool progress, verified results, failures, and pending
  actions.
- Do not infer filesystem intent.
- Do not own permission decisions.

Target UX:

- Natural chat answers for normal questions.
- Compact task progress while tools run.
- One concise summary after a successful project scaffold.
- No duplicate "Created/Wrote" spam for every file unless expanded details are
  requested.
- Clear failed action messages when verification fails.

### Tests And Harness

Status: Partial.

Current state:

- Unit and integration coverage protect the core plain-chat boundary, verified
  plan execution, artifact memory, trace redaction, footer display, and TUI
  rendering helpers.
- `./bin/check-local` is the current broad local safety command.
- The golden/live regression harness is still the biggest missing protection
  layer and should be prioritized before riskier runtime refactors.

Files/modules:

- `crates/elgar-core/src/*tests*`
- `crates/elgar-tui/src/terminal/tests/`
- `crates/elgar-cli/src/*tests*`
- `bin/check-local`
- future golden transcript fixtures

Responsibilities:

- Prove the model-tool loop works without network where possible.
- Prove the plain-chat provider payload has no tools, no `tool_choice`, and no
  project/folder memory.
- Prove real installed TUI smoke for high-risk flows.
- Cover path targeting, permission modes, plan-followup memory, and ambiguity.
- Keep only focused compatibility tests for the remaining small controller
  wrapper.

## Step-by-Step Plan

### Step 1: Freeze The Target Contract

Status: Complete.

Linear:

- `ELG-314`

Deliverable:

- This document becomes the repo-local contract for the next migration work.
- The orchestrator handoff links here.

Done when:

- The team agrees the target contract is the source of truth for runtime
  migration decisions.

### Step 2: Harden AgentRuntime Policy

Status: Complete.

Linear:

- `ELG-315`

Implementation:

- Pass `PermissionPolicyMode` from CLI/TUI into `run_permissive_agent_turn`.
- Rename the loop away from `permissive` if it now enforces policy.
- Replace hardcoded `PermissionPolicyMode::FullAccess` in `agent_loop`.
- Apply mode-specific decisions:
  - `review_all`: propose only, no auto-apply.
  - `auto_create_review_modify`: auto-create new files/directories, gate edits,
    deletes, moves, and shell.
  - `workspace_write_with_review`: allow safe workspace writes, gate risky work.
  - `full_access`: auto-apply after validation and allowed-root checks.

Tests:

- Unit tests for each permission mode.
- TUI script tests showing pending actions only when policy requires review.
- Regression that auto-applied actions are recorded as policy-approved, not
  user-approved.

Done when:

- The runtime never behaves like implicit full access unless the selected mode
  is explicitly `full_access`.

### Step 3: Make Normal Chat Path Unambiguous

Status: Partial.

Implemented:

- Plain/simple chat sends a plain provider request without tools, tool choice,
  folder anchoring, or verified project memory.
- Verified state answers handle common follow-up status questions without
  forcing tool-enabled turns.

Remaining:

- Continue quarantining legacy controller naming/surfaces.
- Keep strengthening regression coverage around route repair and state answers.

Linear:

- Continue under `ELG-304`.

Implementation:

- Ensure normal CLI/TUI text enters `AgentRuntime`.
- Delete legacy controller smoke paths after AgentRuntime owns normal chat.
- Remove or quarantine normal-chat helper paths that still call controller
  routing.
- Update names/comments that imply controller owns normal chat.

Tests:

- Normal greeting uses a plain provider payload with no tools, no
  `tool_choice`, and no latest-folder/project-plan context.
- Capability question does not mutate files.
- Natural creation request uses model tool path and policy.
- Previous project memory followed by `hello` does not inject project/folder
  context or trigger workflow handling.
- Previous project/plan flow followed by `ok` does not create files unless an
  explicit pending confirmation state exists.
- Legacy controller smoke still works only through explicit commands.

Done when:

- No normal live chat path treats the controller as conversational brain.

### Step 4: Stabilize Tool Validation And Targeting

Status: Partial.

Implemented:

- Verified plan scope and follow-up execution are much more reliable.
- Plan execution can create all expected files/folders from the verified plan.
- Off-plan paths are blocked during verified plan execution.

Remaining:

- Fix duplicate workspace-root wording/path display cases such as
  `playground/playground/...`.
- Add intent-scoped tool definitions before the provider call.
- Keep shell-command verification out of the main file-planning reliability
  path until file flows stay solid.

Linear:

- Builds on `ELG-313`.

Implementation:

- Keep path repair narrow and test-backed.
- Normalize Desktop/home/project-root references before execution.
- Treat missing tool arguments as recoverable only when the active target is
  known.
- Ask guidance when target or intent is ambiguous.

Tests:

- Desktop/home folder creation.
- One-shot React/Next/Tailwind project creation.
- Plan then implement in same folder.
- Missing `target_path` recovery.
- No repo-root writes when user requested Desktop/home.

Done when:

- The model can make imperfect tool calls without Elgar corrupting target
  location or silently writing to the wrong root.

### Step 5: Add Golden Transcript Harness

Status: Pending.

Linear:

- `ELG-311`

Implementation:

- Add fake-provider golden transcript tests for:
  - greeting
  - capability answer
  - Desktop/home project creation
  - plan first, implement later
  - ambiguity and guidance
  - failed tool recovery
- Add filesystem assertions for every mutating transcript.
- Add installed TUI smoke for the current highest-risk project scaffold.

Done when:

- Regressions like `Desktop/Desktop`, repo-root Tailwind files, robotic
  duplicate file spam, and false success claims fail tests before reaching the
  user.

### Step 6: Clean UI Reporting Boundaries

Status: Partial.

Implemented:

- Plan previews render as compact status plus tree output.
- TUI footer is quieter and places the model on the right.
- Working/thinking display is simpler than the earlier busy footer.

Remaining:

- Batch more low-level tool/action output into one natural project summary.
- Keep detailed event data available through debug/copy/trace views.

Linear:

- Continue `ELG-302` / `ELG-303` direction or create a focused child issue.

Implementation:

- Render batches as a single project summary by default.
- Keep per-file details available through copy/logs/future expand controls.
- Hide provider tool-chatter from the final transcript.
- Keep reasoning/progress concise and natural while work is running.

Tests:

- Project scaffold transcript shows one concise result.
- Interleaved tool failures do not break grouping.
- Pending action panel shows `none` after policy-applied work.

Done when:

- The TUI feels like a natural coding agent instead of a raw action log.

### Step 7: Rename And Split Only Where It Reduces Confusion

Status: Partial.

Implemented:

- `elgar-cli` has been split into focused modules.
- `elgar-tui` panes and terminal code have been split into focused modules.

Remaining:

- Do the risky `elgar-core/src/agent_loop.rs` split only after the golden
  regression wall is stronger.

Linear:

- Create a cleanup issue only after Steps 2-6 are green.

Implementation:

- Rename controller-owned modules that now serve AgentRuntime.
- Split `agent_loop.rs` only if policy/tool-targeting/provider-turn logic stays
  hard to audit.
- Avoid splitting purely because a file is large.

Suggested split only if needed:

```text
agent_loop.rs          turn orchestration
agent_prompt.rs        system prompt and context assembly
agent_tools.rs         validation/retry/retargeting glue
agent_apply.rs         policy decision and executor dispatch
```

Done when:

- File names match current ownership and the runtime remains easy to inspect.

### Step 8: Update Planning Sources

Status: Partial.

Implemented:

- Repo-local docs now identify `docs/elgar-product-architecture-plan.md` as the
  current product/runtime contract.
- Google Drive planning sources are indexed in
  `zz_elgar_agent_docs/GOOGLE_DRIVE_PLANNING_SOURCES.md`.

Remaining:

- Update or annotate older Google Drive docs that still use controller-first
  language after the current runtime stabilizes.

Linear:

- Add a documentation issue after the runtime is stable.

Implementation:

- Update repo docs first.
- Then update Google planning docs with the final architecture contract.
- Mark old controller-first language as historical where needed.

Done when:

- Future agents do not receive contradictory instructions about controller-first
  versus AgentRuntime-first normal chat.

### Step 9: Design Run Harness / Issue Runner

Status: Pending.

Linear:

- New design issue only after the runtime/harness is stable.

Implementation direction:

- Add a layer above runtime, not inside runtime.
- It should select one Linear issue, build bounded context, run one iteration,
  collect actions/checks, write a ledger, update Linear, and stop.

Possible ledger shape:

```text
.elgar/runs/<run-id>/
  run.json
  summary.md
  checks.json
  actions.jsonl
```

Done when:

- The product can run repeatable issue-focused coding sessions without mixing
  orchestration, chat runtime, and filesystem execution.

## Immediate Recommendation

Do not start MCP, Skills, durable multi-session memory, or the Run Harness yet.

The next implementation issue should be:

```text
ELG-311 Codex-style golden harness and live e2e coverage
```

Reason:

- The core file-planning path is now stable enough to preserve.
- Future changes need repeatable protection before touching riskier runtime
  areas.
- The remaining high-risk work is `agent_loop.rs`, intent-scoped tools, and
  route/performance tuning.

After that, continue `ELG-326` routing/performance work and only then split the
core `agent_loop.rs` module under `ELG-329`.
