# Harness Permissions

This folder owns policy decisions for risky harness behavior.

Permission files should decide whether a requested side effect is allowed,
requires user approval, or must be denied. They should not execute the side
effect themselves.

## Current Files

- `types.rs` defines `Allow`, `NeedsApproval`, and `Deny` decisions.
- `policy.rs` maps validated primitive requests to a permission decision.
- `approval.rs` defines the single pending approval record stored by core.

## Current Behavior

- `read`, `ls`, `find`, and `grep` are allowed.
- `bash`, `write`, and `edit` return `NeedsApproval`.
- `NeedsApproval` creates a pending approval record with id, tool, reason,
  argument preview, and `pending` status.
- Core stores one pending approval slot. A later risky request can replace the
  current pending pointer, while older approval ids remain visible in verified
  evidence and logs.
- No approval command or button exists yet.
- No shell command or file write/edit executes yet.

## Future Files

- `risk.rs` for richer classification of requested operations.
