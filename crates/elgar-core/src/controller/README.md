# elgar-core/src/controller

## Purpose

Reserved for small controller-adjacent modules.

The legacy model-first controller has been removed. Normal chat and tool use
belong to `agent_runtime.rs`; explicit approval and rejection belong to
`action_gate.rs`.

## Checks

- `cargo test -p elgar-core --lib`
