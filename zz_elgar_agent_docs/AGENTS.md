# Elgar v0.2 Agent Instructions

## Purpose

This document defines how coding agents should work on Elgar v0.2.

Elgar v0.2 is a clean restart. Do not preserve v0.1 architecture unless explicitly instructed.

## Core Philosophy

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
Tests protect.
Extensions wait.
```

## Agent Rules

1. Do not copy old v0.1 implementation files blindly.
2. Do not create a giant agent framework.
3. Do not implement MCP, Skills, API, Parallel Agents, Auto Skill Learning, or Obsidian integration in the first slice.
4. Do not implement a full TUI before the Core Harness works.
5. Do not let provider/model text mutate files.
6. Do not let UI code own routing, permissions, or file operations.
7. Keep files small and responsibilities clear.
8. Add tests before expanding behavior.
9. Prefer explicit types over prompt-only behavior.
10. Report what was changed, what was tested, and what was intentionally deferred.

## Agent Roster

Use the stable agent roster in:

```text
AGENT_ROSTER.md
```

Default standing roles:

- Core Harness Agent
- Router/Session Agent
- Action Lifecycle Agent
- Filesystem Safety Agent
- Harness/Test Agent
- Simple TUI Agent
- Provider / LM Studio Agent
- Code Review Agent

Prefer one implementation agent at a time. Reuse these roles rather than creating new standing agents. Use Code Review Agent at risk gates, especially after filesystem mutation work, before completing the Core Harness slice, and before live provider or TUI integration.

## Source of Truth

The canonical planning docs currently live in Google Drive.

Use this exact planning index:

```text
ELGAR_PLANNING_INDEX
https://docs.google.com/document/d/1-V7QT5Au67g20pR5OAzh2_LpAZxIX0NLiUXsl2TW66c/edit
```

Planning index document ID:

```text
1-V7QT5Au67g20pR5OAzh2_LpAZxIX0NLiUXsl2TW66c
```

Also see:

```text
GOOGLE_DRIVE_PLANNING_SOURCES.md
```

Before implementation, use either:

1. the Google Drive planning index above, or
2. exported Markdown copies under `docs/planning/` if they have been added to the repo.

Do not proceed blindly if neither source is available.

Minimum required planning docs:

- `ELGAR_V0_2_PLAN`
- `PRODUCT_PRINCIPLES`
- `CONTROLLER_TRUTH_MODEL`
- `RESPONSIBILITY_BOUNDARIES`
- `CORE_HARNESS_ROADMAP`
- `PERMISSIONED_ACTIONS_ROADMAP`
- `HARNESS_REGRESSION_TESTS_ROADMAP`

If working in the repo and `docs/planning/` is missing, either use the exact Google Drive links in `GOOGLE_DRIVE_PLANNING_SOURCES.md` or stop and report that the planning docs must be exported into the repo before implementation.

## First Implementation Target

Build the Core Harness.

The first useful workflow is:

```text
User asks to create hello.py
→ controller proposes WriteFile
→ user approves or rejects
→ only approved action writes the file
→ filesystem confirms
→ renderer reports truthfully
```

## What to Report Back

Every agent run should report:

- Linear issue worked on
- files created or modified
- architecture decisions made
- tests added
- commands run
- what passes
- what is intentionally deferred
- any blockers or ambiguity
