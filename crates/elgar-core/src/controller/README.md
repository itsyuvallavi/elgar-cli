# elgar-core/src/controller

## Purpose

Legacy controller submodules for explicit review, approval, and compatibility behavior that is too large for `controller.rs`.

## Important Files

- `legacy_controller_model_first.rs` supports old model-first controller paths while normal chat migrates to `agent_runtime.rs`.

## Ownership

Controller modules are not the normal conversational brain. Keep them isolated to explicit review/approval and legacy smoke paths while the agent runtime owns normal chat.

## Checks

- `cargo test -p elgar-core controller`
- `cargo test -p elgar-core model_first`
