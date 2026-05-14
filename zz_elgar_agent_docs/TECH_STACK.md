# Elgar v0.2 Tech Stack

## Language

Recommended initial language: **Rust**

Reason:

- strong type system
- good CLI/TUI ecosystem
- reliable filesystem and process handling
- suitable for local-first tooling
- easier to enforce controller/action boundaries

## Initial Workspace

Recommended workspace:

```text
Cargo.toml
crates/
  elgar-core/
  elgar-cli/
```

Root `Cargo.toml` should define a workspace:

```toml
[workspace]
members = [
  "crates/elgar-core",
  "crates/elgar-cli",
]
resolver = "2"
```

Future crates:

```text
crates/
  elgar-tui/
  elgar-provider/
  elgar-harness/
```

Do not create future crates until needed.

## Core Dependencies

Start with as few dependencies as possible.

Recommended early dependencies:

```text
anyhow or thiserror       error handling
serde                     serialization
serde_json                debug/state snapshots
tempfile                  tests for file actions
uuid or ulid              action/session ids, if needed
```

Delay adding:

```text
ratatui                   until TUI begins
crossterm                 until TUI begins
reqwest                   until live LM Studio provider begins
tokio                     only when async provider/TUI needs it
clap                      when CLI needs structured commands
```

## First Core Modules

```text
elgar-core/src/
  lib.rs
  event.rs
  action.rs
  session.rs
  router.rs
  provider.rs
  controller.rs
  fs.rs
  renderer.rs
```

## Provider Strategy

Start with a provider stub.

Then add LM Studio through an OpenAI-compatible provider path.

Do not start with multiple providers, model routing, planner/coder roles, or compatibility matrices.

## TUI Strategy

Use the TUI only after the Core Harness proves the action lifecycle.

Likely future TUI stack:

```text
ratatui
crossterm
```

The TUI must call the same controller as the CLI.

## Deferred Technology

Do not add yet:

- MCP
- API server
- Obsidian integration
- Skills runtime
- Parallel Agents / Swarm
- Auto Skill Learning
- complex eval harness
- plugin system
