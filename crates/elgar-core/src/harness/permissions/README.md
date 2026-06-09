# Harness Permissions

This folder will own policy and approval checks for risky harness behavior.

Permission files should decide whether a requested side effect is allowed,
requires user approval, or must be denied. They should not execute the side
effect themselves.

Future files may include:

- `policy.rs` for permission-mode decisions.
- `approval.rs` for user approval flow state.
- `risk.rs` for classifying requested operations.

