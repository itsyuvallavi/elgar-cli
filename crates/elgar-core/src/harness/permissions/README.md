# Harness Permissions

This folder owns policy decisions for risky harness behavior.

Permission files should decide whether a requested side effect is allowed,
requires user approval, or must be denied. They should not execute the side
effect themselves.

## Current Files

- `types.rs` defines `Allow`, `NeedsApproval`, and `Deny` decisions.
- `policy.rs` maps validated primitive requests to a permission decision.

## Current Behavior

- `read`, `ls`, `find`, and `grep` are allowed.
- `bash`, `write`, and `edit` return `NeedsApproval`.
- No approval prompt exists yet.
- No shell command or file write/edit executes yet.

## Future Files

- `approval.rs` for user approval flow state.
- `risk.rs` for richer classification of requested operations.
