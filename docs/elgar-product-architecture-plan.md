# Elgar Product Architecture Plan

Date: 2026-05-25

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

## Project Structure From Now On

Keep the project organized by ownership boundaries, not by historical
implementation order.

### Core Runtime

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

Near-term cleanup:

- `AgentRuntime::turn` must use the selected `PermissionPolicyMode`.
- `agent_loop` must stop hardcoding `FullAccess`.
- Permission policy must decide create/edit/delete/move/shell behavior.

### Executors

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

Files/modules:

- `crates/elgar-core/src/*tests*`
- `crates/elgar-tui/src/terminal/tests/`
- `crates/elgar-cli/src/*tests*`
- `bin/check-local`
- future golden transcript fixtures

Responsibilities:

- Prove the model-tool loop works without network where possible.
- Prove real installed TUI smoke for high-risk flows.
- Cover path targeting, permission modes, plan-followup memory, and ambiguity.
- Keep legacy controller tests only where legacy compatibility still exists.

## Step-by-Step Plan

### Step 1: Freeze The Target Contract

Linear:

- `ELG-314`

Deliverable:

- This document becomes the repo-local contract for the next migration work.
- The orchestrator handoff links here.

Done when:

- The team agrees the target contract is the source of truth for runtime
  migration decisions.

### Step 2: Harden AgentRuntime Policy

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

Linear:

- Continue under `ELG-304`.

Implementation:

- Ensure normal CLI/TUI text enters `AgentRuntime`.
- Keep legacy controller smoke paths explicitly named.
- Remove or quarantine normal-chat helper paths that still call controller
  routing.
- Update names/comments that imply controller owns normal chat.

Tests:

- Normal greeting uses model/provider path.
- Capability question does not mutate files.
- Natural creation request uses model tool path and policy.
- Legacy controller smoke still works only through explicit commands.

Done when:

- No normal live chat path treats the controller as conversational brain.

### Step 4: Stabilize Tool Validation And Targeting

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

Do not start the Run Harness yet.

The next implementation issue should be:

```text
ELG-315 Harden AgentRuntime permission policy enforcement
```

Reason:

- Normal chat already moved toward AgentRuntime.
- The current risk is that AgentRuntime still behaves like implicit full access
  in parts of the loop.
- Golden harness work should test the final policy contract, not the temporary
  permissive behavior.

After that, run `ELG-311` as the regression wall.
