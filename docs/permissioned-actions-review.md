# Permissioned Actions Review

Date: 2026-05-20

Scope reviewed: ELG-216 through ELG-219.

## Result

The expanded permissioned action slice was originally reviewed against the v0.2 controller boundary. Current normal chat should enter through `AgentRuntime`; explicit approvals go through the action gate.

- Core owns action lifecycle and verified truth.
- Model/provider text can suggest but cannot approve, execute, mutate, or verify.
- User approval is required before filesystem mutation or shell execution.
- Filesystem confirms file truth for create, edit, overwrite, delete, move, and directory actions.
- Shell executor confirms command truth for approved shell actions.
- UI and CLI render core events and results without owning mutation or execution.
- Default checks remain no-network and no-model.

## Covered Behaviors

File actions:

- Proposed create/edit/overwrite/delete/move/directory actions do not mutate.
- Rejected actions remain terminal and do not mutate.
- Approved actions record verified filesystem results.
- Allowed-root, traversal, existing-target, and symlink safety remain covered.

Shell actions:

- Proposed shell commands do not execute.
- Rejected shell commands do not execute.
- Approved shell commands execute once through the action gate path.
- Shell results record stdout, stderr, exit code, elapsed time, timeout status,
  and output truncation flags.
- Timeout and capped-output paths are covered.

Provider boundary:

- Provider prose cannot create, approve, reject, execute, apply, or verify
  pending file or shell actions.
- Provider output remains suggestion text unless the controller records a
  separate action/result event.

UI boundary:

- TUI and CLI route approval/rejection through the narrow action gate.
- Rendering pending actions does not mutate files or run commands.
- Shell results render as verified core results.

## Known Gaps

- Shell commands run via opaque `sh -c` after approval.
- Shell environment policy is inherited controller environment only.
- Shell output stores capped text plus truncation flags, not full uncapped
  output.
- Create-directory remains single-level, not recursive.
- Simple parsers do not support quoted paths or paths with spaces.

These are acceptable for the completed permissioned-action slice and should be
handled only by explicit follow-up issues.

## Recommended Next Slice

Use `ELG-92` to define the Obsidian/read-first memory roadmap, then create a
small implementation issue for read-only local memory ingestion once the scope
is clear.

Reasoning:

- Permissioned mutation/execution boundaries are now covered.
- Extensions still wait, so the next slice should be read-first and local.
- Obsidian memory can build on core context accounting without
  introducing write access, live provider requirements, or external network
  dependencies.
