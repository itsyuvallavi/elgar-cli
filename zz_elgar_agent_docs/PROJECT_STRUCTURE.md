# Elgar v0.2 Project Structure

## Goal

Keep the project small and readable at the beginning.

The first implementation should prove the Core Harness, not the full product.

## Recommended Initial Structure

```text
elgar/
  Cargo.toml
  README.md
  AGENTS.md

  docs/
    planning/
      ELGAR_V0_2_PLAN.md
      PRODUCT_PRINCIPLES.md
      CORE_WORKFLOWS.md
      NON_GOALS_AND_SUCCESS_CRITERIA.md
      CONTROLLER_TRUTH_MODEL.md
      RESPONSIBILITY_BOUNDARIES.md
      EXTENSION_BOUNDARIES.md
      CORE_HARNESS_ROADMAP.md
      PROVIDER_LM_STUDIO_ROADMAP.md
      PERMISSIONED_ACTIONS_ROADMAP.md
      SIMPLE_TUI_ROADMAP.md
      HARNESS_REGRESSION_TESTS_ROADMAP.md

  crates/
    elgar-core/
      Cargo.toml
      src/
        lib.rs
        event.rs
        action.rs
        session.rs
        router.rs
        provider.rs
        controller.rs
        fs.rs
        renderer.rs

    elgar-cli/
      Cargo.toml
      src/
        main.rs

  tests/
    fixtures/
```

## Root Cargo Workspace

The repo must include a root `Cargo.toml` so workspace commands are unambiguous.

Minimal expected root workspace:

```toml
[workspace]
members = [
  "crates/elgar-core",
  "crates/elgar-cli",
]
resolver = "2"
```

This enables commands like:

```text
cargo check --workspace
cargo test --workspace
```

## Future Structure

Only after the Core Harness is stable:

```text
crates/
  elgar-tui/
  elgar-provider/
  elgar-harness/
```

Only much later:

```text
crates/
  elgar-obsidian/
  elgar-skills/
  elgar-mcp/
  elgar-api/
```

## File Size Rule

Avoid thousand-line files.

Suggested soft limits:

```text
core module: under 300 lines
tests per feature: under 400 lines
TUI file: split early by app/state/view/input
```

## Ownership Boundaries

```text
router.rs       classifies input
controller.rs   owns truth and state transitions
action.rs       defines proposed/approved/applied actions
session.rs      stores session state
provider.rs     model interface/stub
fs.rs           verified filesystem operations
renderer.rs     plain text rendering of events/results
event.rs        renderable event types
```

## Forbidden Early Structure

Do not create:

```text
agent.rs
orchestrator.rs with subagents
mcp/
skills/
api/
obsidian/
```

unless the planning gate explicitly approves it later.
