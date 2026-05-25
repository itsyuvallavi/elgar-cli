# Model-First Routing Plan

Linear issue: ELG-258

Status note, May 24 2026: the live TUI runtime has moved past this
controller-mediated model-first plan. Normal live TUI turns now use the
Pi-style agent/tool loop from ELG-293. The controller model-first path described
here is retained only as legacy/controller-review behavior while ELG-297
quarantines remaining old-runtime coupling. Normal live TUI turns must continue
to call `agent_loop::run_permissive_agent_turn`; controller-review model-first
entry points are compatibility and smoke-test surfaces only.

## Purpose

This document defines the target architecture for moving Elgar from
deterministic natural-language routing toward model-first routing with an
explicit permission policy contract.

This is an architecture plan, not an implementation change.

## Philosophy

Target philosophy:

```text
Model owns intent.
Policy decides.
Model acts through tools.
Filesystem records and verifies.
UI reports.
Tests protect.
```

This updates the shape of the current v0.2 loop without weakening controller
truth. The model may own intent detection and tool selection, but it still does
not own permission, filesystem truth, command truth, or UI truth.

In practice:

- The model interprets natural language and drafts tool calls.
- The permission policy decides whether a drafted tool call may apply, needs
  review, or must be rejected.
- The controller validates model drafts into typed actions or tools.
- Filesystem and shell layers perform only allowed, controller-mediated work.
- The UI reports controller events and verified results.
- Tests protect every safety boundary.

## Pi-Like Inspiration

The inspiration is Pi-like in interaction shape: a smooth model/tool loop where
conversation can produce useful actions without making the user speak in rigid
commands.

Elgar should not copy Pi's identity, voice, or hidden autonomy. Elgar keeps:

- controller-owned truth
- typed actions and tool calls
- filesystem verification
- visible pending review when policy requires it
- configurable permission policy modes
- no-network and no-provider regression tests

The goal is a more natural interface, not a less accountable system.

## Current Problem

Today, both the TUI and core participate in natural-language routing.

Concrete examples:

- `crates/elgar-core/src/router.rs` classifies many natural-language phrases
  into action routes.
- `crates/elgar-core/src/controller.rs` branches on those routes and builds
  action proposals.
- `crates/elgar-tui/src/terminal/commands.rs` has its own text heuristics for
  deciding whether input starts a provider turn or should stay on the controller
  action path.

This creates several problems:

- Natural-language behavior depends on brittle phrase rules.
- The TUI can drift toward owning routing decisions.
- Users must phrase requests in ways the deterministic parser recognizes.
- Copy becomes robotic because the system is optimized around command-like
  phrases instead of intent.
- Adding new action types means expanding parser rules instead of tool schemas
  and policy decisions.

The deterministic router has been useful for the core harness, but it should
not remain the main natural-language brain.

## Target Architecture

Routing should split into two lanes.

### Deterministic Command Lane

Keep deterministic routing only for explicit local commands:

- slash commands such as `/help`, `/clear`, `/approve`, `/reject`, `/memory`,
  `/copy`, and `/exit`
- pending approval commands such as approve, reject, cancel, or selecting a
  pending action
- strict CLI or test harness commands where deterministic behavior is the
  product contract

This lane must stay small and auditable.

### Model-First Natural-Language Lane

All ordinary natural language should go model-first:

```text
user text
-> controller starts model turn
-> model drafts answer and/or tool call
-> controller validates draft
-> permission policy decides
-> action/tool applies only when allowed
-> filesystem/shell verifies
-> UI reports controller truth
```

The model owns intent detection in this lane. The controller owns validation,
policy, action state, execution orchestration, and verified truth.

## Model Tool-Call Protocol

Model tool calls are untrusted drafts until validated.

A draft tool call should include:

- tool name
- arguments
- short user-facing summary
- expected effect
- target paths or command cwd when applicable
- risk notes when known

The controller validates a draft before it can become an action or tool
execution:

- tool name is known
- arguments parse into the typed schema
- target paths resolve under allowed roots
- action kind is supported
- request does not conflict with current pending actions
- request satisfies permission policy
- request can be represented as an auditable action/result

Validation failure produces a controller error or assistant clarification. It
must not fall through to execution.

Provider prose remains suggestion text. Only validated tool calls can become
actions, and only policy-approved actions can mutate files or run commands.

## Permission Policy Contract

The permission policy is a controller-owned contract that maps a validated tool
request to one of these decisions:

```text
allow_apply
require_review
reject
```

The decision should include:

- policy mode
- action kind
- target
- reason
- approval source
- policy decision metadata
- whether filesystem verification is required
- whether user approval is required

Policy decisions are not made by the model, the TUI, or provider text.
Auto-create is a policy-approved auto-apply path after validation. It is not a
filesystem bypass and must not skip controller action records, allowed-root
checks, or filesystem verification.

Audit records must distinguish explicit user approval from policy approval.
Auto-approved work should be recorded as policy-approved with its policy mode,
decision reason, and approval source. It must not be hidden or rendered as if
the user manually approved it.

## Policy Modes

### `review_all`

Every mutating action requires review.

Use for the most conservative local mode and for early migration tests.

### `auto_create_review_modify`

New files and directories inside allowed roots may auto-apply after policy
validation and filesystem verification.

Existing-file edits, overwrites, deletes, moves, and shell commands require
review.

This is the recommended first target because it improves natural creation
workflows while preserving strict review for destructive or ambiguous work.

### `workspace_write_with_review`

File writes inside allowed roots may apply automatically when the policy marks
them safe. Higher-risk operations still require review.

This mode should wait until edit/patch validation is stronger than it is today.

### `full_access`

Allowed actions may apply without review after validation.

This mode should exist as an explicit future contract, not as an early default.
It still must respect allowed roots, typed validation, filesystem verification,
and shell policy. It must not mean arbitrary host access.

## Recommended First Target

Implement `auto_create_review_modify` first.

Rules:

- Creating a new project-relative file inside an allowed root can auto-apply
  after validation.
- Creating a new project-relative directory inside an allowed root can
  auto-apply after validation.
- The filesystem must verify the created path before the UI reports success.
- If the target already exists, the action becomes review-gated or rejected by
  policy.
- Absolute-path requests, shell-backed creation, existing-file edits,
  overwrites, patches, deletes, moves, and shell commands remain approval-gated
  or rejected initially.

This gives the model-first lane a useful success path without silently
modifying existing work.

## Auto-Create Rules

Auto-create applies only when all of these are true:

- Policy mode allows auto-create.
- The request is `CreateFile` or `CreateDirectory`.
- The target is a new project-relative path that resolves inside an allowed
  root.
- The target path does not already exist.
- Parent directory rules are explicit and validated.
- The action is represented as a typed controller action or tool result.
- The filesystem operation succeeds.
- Verification confirms the expected file contents or directory existence.

Auto-create does not apply to:

- existing-file edits
- overwrites
- patches
- deletes
- moves or renames
- shell commands
- shell-backed creation
- absolute-path requests
- symlink escapes
- paths outside allowed roots
- ambiguous multi-action plans
- provider prose without a validated tool call

## First-Slice Multi-Tool Behavior

The first implementation slice should accept at most one tool call per model
turn. This keeps policy decisions, pending review state, and UI reporting easy
to audit while the model-first lane is being introduced.

If a model returns multiple tool calls in one turn, the controller must handle
that safely. It should reject the batch or ask for clarification. It must not
partially apply a mixed batch by accident.

Future batch support needs an explicit design before implementation. That
design should define ordering, rollback expectations, per-tool policy
decisions, audit records, and UI reporting. Mixed auto-approved and
review-gated batches are deferred.

## Safety Invariants

These must remain true in every mode:

- Provider prose cannot mutate files.
- Provider prose cannot run shell commands.
- Provider prose cannot approve an action.
- The TUI cannot own permission decisions.
- The TUI cannot own file or shell truth.
- A policy decision cannot bypass typed validation.
- A tool call cannot apply if its path escapes allowed roots.
- Existing files cannot be overwritten by an auto-create action.
- Deletes and moves cannot auto-apply in the first target mode.
- Shell commands cannot auto-run in the first target mode.
- UI success text must wait for controller-recorded verification.
- Tests must cover model claims that falsely imply mutation.

What cannot happen:

- A model says "I wrote the file" and Elgar treats that as true.
- A natural-language parser silently rewrites an existing file.
- A slash command or TUI shortcut bypasses the controller.
- A failed verification is reported as success.
- A policy mode grants access outside the configured workspace roots.

## Affected Files And Modules

Likely affected modules for implementation:

- `crates/elgar-core/src/router.rs`: shrink natural-language routing to the
  deterministic command lane.
- `crates/elgar-core/src/controller.rs`: route natural language through the
  model-first tool-call flow, validate drafts, ask policy, and record events.
- `crates/elgar-core/src/action.rs`: keep typed action payloads as the
  auditable action contract; add policy metadata only if needed.
- `crates/elgar-core/src/fs.rs`: keep allowed-root resolution and verification
  as the file truth layer.
- `crates/elgar-core/src/shell.rs`: keep shell execution approval-gated for the
  first target.
- `crates/elgar-core/src/provider/`: expose or parse structured model tool-call
  drafts without treating provider text as truth.
- `crates/elgar-core/src/session.rs`: store policy mode, decisions, pending
  actions, and verified results if not already represented.
- `crates/elgar-core/src/event.rs`: report policy decisions, auto-applied
  results, review-required actions, and validation failures.
- `crates/elgar-tui/src/terminal/commands.rs`: keep slash command parsing, but
  remove natural-language provider/controller heuristics.
- `crates/elgar-tui/src/shell.rs`: continue submitting input through the
  controller and rendering controller events.
- `crates/elgar-cli/src/main.rs` and `crates/elgar-cli/src/lib.rs`: expose
  policy mode configuration and keep CLI behavior on the same controller path.
- `crates/elgar-core/tests/core_harness_regression.rs` and TUI smoke tests:
  add no-network coverage for the new lane split and policy decisions.

## Migration Phases

### Phase 1: Contract And Types

Add a small permission policy type and decision result.

Tests after phase:

- policy modes serialize/parse if exposed in config
- `review_all` requires review for every mutating action
- `auto_create_review_modify` allows only new file/directory creation
- existing targets require review or reject
- paths outside allowed roots reject
- policy decisions record approval source and decision metadata
- auto-approval is not recorded or rendered as user approval

### Phase 2: Tool-Call Draft Validation

Introduce model tool-call draft parsing and validation into typed actions.

Tests after phase:

- unknown tool name rejects
- malformed arguments reject
- path traversal rejects
- absolute-path auto-create rejects or requires review
- provider prose alone does not create actions
- valid create-file draft becomes a typed action
- valid shell-command draft remains review-gated
- multiple tool calls in one model turn reject or require clarification

### Phase 3: Model-First Natural-Language Lane

Route ordinary natural language through the model-first controller path.

Tests after phase:

- natural create request reaches provider/tool-call path
- deterministic phrase parser no longer owns natural create/edit/delete
- slash commands still work
- approve/reject still target pending actions
- unknown natural language gets model answer or clarification, not parser copy

### Phase 4: Auto-Create Apply Path

Enable auto-apply for new files and directories under
`auto_create_review_modify`.

Tests after phase:

- new file auto-applies and verifies
- new directory auto-applies and verifies
- auto-create records policy approval source and decision metadata
- existing file does not auto-overwrite
- existing directory does not auto-overwrite
- patch, move, delete, overwrite, shell-backed, and absolute-path requests do
  not auto-create
- failed filesystem verification reports failure
- UI text reports only verified controller truth

### Phase 5: TUI Cleanup

Remove TUI natural-language routing heuristics and leave only explicit terminal
commands.

Tests after phase:

- `/help`, `/clear`, `/approve`, `/reject`, `/memory`, `/copy`, and `/exit`
  remain deterministic
- ordinary text is submitted to the controller model-first path
- TUI does not call provider directly
- TUI does not mutate files directly
- pending review actions render from controller events

### Phase 6: No-Network End-To-End Gate

Add a no-network e2e flow using a deterministic provider stub that emits tool
call drafts.

Tests after phase:

- model-first create file auto-applies in `auto_create_review_modify`
- model-first edit requires review
- model-first delete requires review
- model-first shell command requires review
- provider false success claim is ignored without verification
- CLI and TUI use the same controller path

Optional live TUI verification should run only with Yuval approval. Live tests
must stay opt-in and must not become part of the default local gate.

## Test Strategy

Default checks should remain no-network and deterministic:

- policy unit tests
- policy decision audit tests for approval source and decision metadata
- tool-call draft validation tests
- multi-tool behavior tests proving first-slice rejection or clarification
- controller flow tests with provider stub
- filesystem verification tests
- TUI smoke tests over controller events
- CLI/TUI boundary tests proving the same controller path

Live provider and live TUI tests are optional. They should require an explicit
environment flag or direct Yuval approval before running.

## Open Decisions

- Exact provider tool-call format for LM Studio-compatible models.
- Whether policy mode lives in `Session`, provider config, CLI args, TUI
  settings, or a small runtime config object.
- Whether auto-create should create missing parent directories or require parent
  directories to already exist at first.
- Exact shape of future batch tool-call support, including mixed auto-approved
  and review-gated batches.

## Next Recommended Issue

Create a small implementation issue before behavior changes for type-only
`PolicyDecision` and `ApprovalSource` support. It should add the policy decision
contract, approval-source audit metadata, and no-network tests, without changing
natural-language routing or auto-apply behavior yet.
