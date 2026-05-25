# Codex-Style Agent Runtime Migration Plan

Date: 2026-05-25

## Goal

Make Elgar's normal chat path behave like a permissive coding agent instead of a controller-gated workflow.

The controller must stop being the conversational brain. Normal TUI/CLI chat should submit a user turn to an agent runtime, the model should reason and call tools, policy should decide whether a tool can run, the filesystem should verify results, and the UI should render concise events.

## Codex Reference

Reviewed OpenAI Codex Rust architecture:

- `codex-rs/protocol/src/protocol.rs`
  - Uses a submission queue / event queue pattern.
  - `Op::UserInput` carries user turns and turn-scoped settings.
  - approval and patch decisions are separate operations, not natural-language reroutes.
- `codex-rs/core/src/agent/control.rs`
  - Core owns turn control and tool execution.
  - UI does not classify user requests into filesystem actions.
- `codex-rs/core/src/tools`
  - Tool execution is a runtime concern with policy and verification.
- `codex-rs/tui/src/chatwidget.rs`
  - TUI is an event renderer/composer. It submits ops and renders streaming/tool state.
  - Slash command parsing is local UI behavior, not the normal chat path.
- `codex-rs/core/src/config` and `codex-rs/protocol/src/permissions.rs`
  - Permission profile, approval policy, sandbox settings, cwd, and workspace roots are explicit runtime configuration.

Reference links:

- https://github.com/openai/codex/tree/main/codex-rs
- https://github.com/openai/codex/blob/main/codex-rs/protocol/src/protocol.rs
- https://github.com/openai/codex/tree/main/codex-rs/core/src/agent
- https://github.com/openai/codex/tree/main/codex-rs/core/src/tools
- https://github.com/openai/codex/blob/main/codex-rs/tui/src/chatwidget.rs

## Diagnosis In Elgar

Normal live TUI is improved but still structurally wrong:

- `crates/elgar-tui/src/terminal.rs` still accepts `Controller<P>` for normal chat.
- `crates/elgar-tui/src/terminal/provider_task.rs` calls `run_permissive_agent_turn`, but only by pulling `controller.provider`.
- `crates/elgar-tui/src/shell.rs` still has `submit_model_first_input(controller, ...)`.
- `crates/elgar-cli/src/lib.rs` creates a `Controller::default()` for TUI script mode and passes it into normal chat.
- `crates/elgar-core/src/controller.rs` still owns legacy natural-language routing and approval behavior.

That means the live path can regress easily: a normal sentence can still be close to `route_input`, approval commands, controller-owned project execution, and controller-specific phrasing.

## Target Architecture

### 1. AgentRuntime Is The Normal Brain

Add `AgentRuntime<P>` in `elgar-core`.

Responsibilities:

- own provider access for normal chat;
- run the model/tool loop;
- return verified events;
- receive `PermissionPolicyMode`;
- never route ordinary text through `route_input`.

The existing permissive loop becomes an implementation detail of `AgentRuntime`, not something the TUI reaches through `Controller`.

### 2. Controller Becomes Legacy Or Explicit Review Only

`Controller` may stay temporarily for old approval/review tests, but it must be quarantined:

- no normal live TUI chat depends on it;
- no normal CLI chat depends on it;
- no project creation path depends on controller routing;
- any remaining use is named `legacy` or `review` in code and tests.

### 3. TUI Renders Runtime Events

TUI should:

- collect user input;
- handle slash commands locally;
- submit normal text to `AgentRuntime`;
- render provider, tool, filesystem, and final answer events.

TUI should not:

- infer filesystem intent;
- create controller action proposals for ordinary text;
- print duplicate tool logs as the final answer;
- expose raw model reasoning or malformed tool syntax.

### 4. Permission Profiles Are Runtime Configuration

Keep the permission levels already added, but make them runtime policy:

- `review_all`
- `auto_create_review_modify`
- `workspace_write`
- `full_access`

The mode must be visible and toggleable in TUI/config, but normal create-file/create-directory under allowed roots should not require the old approval dance when policy allows it.

### 5. Path Resolution Is A Tool Runtime Concern

Create a single path resolver used by tool application:

- `~/foo` resolves to home;
- `Desktop/foo` resolves to `$HOME/Desktop/foo`;
- `same folder`, `last folder`, and follow-up requests resolve from verified session memory;
- ambiguous targets cause one clarification question;
- failed tool calls must not silently retarget to repo root.

### 6. Tool Rendering Is Concise

Tool execution should report batch summaries:

- good: `Created ~/demo-nextjs with 10 files.`
- bad: one line per low-level write plus duplicated final prose.

Detailed tool events can stay in structured history/debug mode, but the normal chat transcript should be human-readable.

### 7. Memory Is Explicit Context, Not Control Flow

Memory should help the model understand context, not override routing:

- verified session memory for current-turn follow-ups;
- future `USER.md` and `MEMORY.md` frozen snapshot layer;
- no transcript dumping;
- no controller-owned hidden project assumptions.

## Execution Plan

### Phase 0: Checkpoint

Done in commit `ecfa4cc` (`Checkpoint model-first TUI runtime fixes`).

### Phase 1: Introduce AgentRuntime And Move Live TUI To It

1. Add `crates/elgar-core/src/agent_runtime.rs`.
2. Export it from `elgar-core`.
3. Change `terminal/provider_task.rs` to accept `AgentRuntime<P>`.
4. Change live terminal functions to construct `AgentRuntime`, not `Controller`, for normal chat.
5. Add `TuiShell::submit_agent_input`.
6. Keep old approval/review methods only for explicit legacy paths.
7. Add focused tests proving normal TUI text does not require `Controller`.

### Phase 2: Move CLI Script Mode To AgentRuntime

1. Change `run_tui_loop_with_policy` to construct `AgentRuntime`.
2. Change `submit_tui_input` to submit normal text through runtime.
3. Keep slash commands local.
4. Leave `controller-smoke` command as explicitly legacy.

### Phase 3: Centralize Path Resolution

1. Extract path resolution from `agent_loop.rs` into a focused module.
2. Add tests for home, desktop, same-folder, existing-folder, and ambiguous-folder cases.
3. Block repo-root fallback when the user explicitly asked for home/desktop.

### Phase 4: Policy-Gated Tool Runtime

1. Replace hardcoded `FullAccess` in permissive apply with policy decisions from `PermissionPolicyMode`.
2. Auto-allow creates when policy allows.
3. Review edits/deletes/shell in stricter modes.
4. Keep full-access mode fully permissive.

### Phase 5: Human Transcript Rendering

1. Batch low-level file actions into one visible summary.
2. Hide raw tool-call syntax from normal transcript.
3. Keep detailed event data available for debug/copy mode.
4. Add golden transcript tests.

### Phase 6: Quarantine Legacy Controller

1. Rename legacy controller paths/tests to make their status explicit.
2. Remove normal-chat imports of `Controller`.
3. Keep old controller smoke commands only while useful for regression comparison.
4. Delete or archive unused legacy controller modules after parity tests pass.

### Phase 7: Harness And E2E

1. Fake-provider transcript tests for:
   - greeting;
   - capability answer;
   - create folder in home;
   - create folder on Desktop;
   - plan then implement in same folder;
   - clarify instead of guessing.
2. Filesystem verification tests.
3. TUI script tests.
4. Installed live TUI smoke after the local test suite passes.

## Acceptance Criteria

Elgar passes when:

- `hello` returns natural chat, no tool/action noise.
- `what can you do?` answers naturally and accurately.
- `create a folder called X in ~/` creates `/Users/yuval/X`.
- `create a folder called X on the desktop` creates `/Users/yuval/Desktop/X`.
- `create a Next.js TS Tailwind project in ~/called X` creates a complete starter under `/Users/yuval/X`.
- Follow-up `add the missing files` writes inside the verified project, not repo root.
- The final answer is one concise summary, not duplicated file-by-file logs.
- Normal live TUI chat has no dependency on controller routing.

## Work Not Included Yet

- Full `USER.md` / `MEMORY.md` implementation.
- Removing every controller test in one pass.
- Replacing the provider client stack.
- Implementing full Codex protocol compatibility.
