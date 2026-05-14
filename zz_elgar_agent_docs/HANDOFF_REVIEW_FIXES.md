# Handoff Review Fixes

This revision addresses the initial review feedback.

## Fixed P1 — Missing source-of-truth planning docs

The handoff now clarifies that the canonical planning docs currently live in Google Drive.

Agents should use the Google Docs planning index or exported Markdown copies under `docs/planning/`.

If neither source is available, implementation is blocked and the agent should report that the planning docs must be added/exported first.

## Fixed P2 — Missing root Cargo workspace

`PROJECT_STRUCTURE.md` and `TECH_STACK.md` now explicitly include the root `Cargo.toml`.

Expected root workspace:

```toml
[workspace]
members = [
  "crates/elgar-core",
  "crates/elgar-cli",
]
resolver = "2"
```

## Fixed P2 — Rejected action lifecycle

`FIRST_DEMO_SPEC.md` now states that rejected actions are terminal.

If the user changes their mind, Elgar must create a new proposal and require approval for the new action. A rejected action must never later mutate the filesystem.


## Fixed follow-up P2 — Google Drive source not identifiable

Added `GOOGLE_DRIVE_PLANNING_SOURCES.md` with the exact planning index URL and every relevant Google Doc URL.

Canonical planning index:

```text
https://docs.google.com/document/d/1-V7QT5Au67g20pR5OAzh2_LpAZxIX0NLiUXsl2TW66c/edit
```

Updated `AGENTS.md`, `CORE_HARNESS_IMPLEMENTATION_PROMPT.md`, and `README_FOR_CODEX.md` to point agents to the exact index and source list.
