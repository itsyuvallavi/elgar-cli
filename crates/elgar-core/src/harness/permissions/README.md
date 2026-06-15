# Harness Permissions

This folder owns policy decisions for risky harness behavior.

Permission files should decide whether a requested side effect is allowed,
requires user approval, or must be denied. They should not execute the side
effect themselves.

## Current Files

- `types.rs` defines `Allow`, `NeedsApproval`, and `Deny` decisions.
- `policy.rs` maps validated primitive requests to a permission decision.
- `approval.rs` defines the single pending approval record stored by core.
- `approval_preview.rs` builds pre-approval display metadata for risky
  filesystem targets.
- `approval_flow.rs` handles `/approve` and `/deny` style commands and
  dispatches approved risky primitives.
- `approval_logging.rs` writes approval decision and approved execution logs.
- `approved_bash.rs` executes approved shell commands in the resolved launch
  folder.
- `approved_write.rs` creates or overwrites one approved file.
- `approved_edit.rs` applies exact one-file text replacement after approval.
- `approved_paths.rs` resolves approved file targets and rejects symlink paths.
- `approved_text.rs` extracts validated argument strings for approved
  execution.

## Current Behavior

- `read`, `ls`, `find`, and `grep` are allowed.
- In the default `review_all` mode, `bash`, `write`, and `edit` return
  `NeedsApproval`.
- In the explicit `workspace_write` mode, safe relative `write` requests inside
  the launch folder can execute immediately. `bash`, `edit`, absolute paths,
  parent paths, symlink paths, and outside-folder writes do not bypass approval
  or execution checks.
- In the explicit `full_access` mode, trusted launch-folder `write`, `edit`,
  and `bash` requests can execute immediately. It is not a shell sandbox; use it
  only for trusted local dogfoods/project generation.
- `NeedsApproval` creates a pending approval record with id, tool, reason,
  exact validated request, argument preview, optional target preview, and
  `pending` status.
- Core stores one pending approval slot. The harness loop stops once a single
  risky request creates pending approval, so provider prose cannot imply that
  additional unrequested side effects are also approved.
- Risky batches emitted in one provider response create one batch approval with
  multiple approved steps.
- A `write` request for a safe relative path that already contains the exact
  requested content is treated as a verified no-op instead of asking for
  approval again.
- Auto-executed `workspace_write` writes return `VERIFIED_WRITE_EXECUTION` with
  `approval_source: workspace_write` and are logged in system/session JSONL.
- Auto-executed `full_access` writes, edits, and bash return verified execution
  output with `approval_source: full_access` and are logged in system/session
  JSONL.
- Pending `write` and `edit` approvals show whether the requested target is
  relative or absolute and whether it appears inside or outside the launch
  folder. This is a display warning only; final symlink and path checks still
  happen during approved execution.
- `/approve` executes approved `bash`, `write`, and `edit` requests.
- `/deny` and `/reject` deny and clear the pending approval.
- Approved `bash` runs the exact approved shell command with `sh -c` in the
  resolved current working directory. It is not sandboxed; approval must treat
  it as arbitrary shell execution.
- Approved `write` writes exact `content` to one file and rejects symlink paths.
- Approved `edit` replaces exact `old_text` with `new_text` only when the old
  text appears exactly once.

## Future Files

- `risk.rs` for richer classification of requested operations.
