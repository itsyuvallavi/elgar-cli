# Elgar Docs

## Purpose

Current source-of-truth documentation for the simplified Elgar rebuild.

Archived docs are reference only. Do not treat files under `archive/` as active
architecture unless a current doc explicitly points to them.

## Start Here

- `PROJECT_PLAN.md` explains the rebuild from the harness baseline to a full agent.
- `ARCHITECTURE.md` explains how Elgar works today.
- `FILE_MAP.md` explains the repo folders and important files.
- `LOCAL_CHECKS.md` explains verification commands.
- `HARNESS_BASELINES.md` records manual harness performance baselines.
- `HARNESS_SHORT_TERM_MEMORY.md` explains the current short-term harness memory
  problem and design direction.
- `NATIVE_TOOL_LOOP.md` defines the target Codex/Pi/Claude-style native tool
  loop and audits the current harness against it.
- `PROVIDER.md` explains LM Studio/provider setup.
- `LOGGING.md` explains local logs.
- `TUI.md` explains the terminal UI path.
- `TOOL_CAPABILITY_MODEL.md` explains how model-visible tools, runtime
  validation, policy, verified execution, and synthesis should fit together.

## Folders

- `agent/` contains agent instructions and handoff history.
- `archive/` contains stale or historical plans.
- `maps/` contains generated HTML/JSON architecture maps.

## Rule

Keep active docs short, current, and readable. Move outdated plans to
`archive/` instead of letting them compete with current docs.
