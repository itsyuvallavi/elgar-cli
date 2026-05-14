# README for Codex

You are working on Elgar v0.2.

This is a clean restart.

## Your First Job

Do not build the whole product.

Start with the Core Harness.

## Read First

1. `AGENTS.md`
2. `AGENT_ROSTER.md`
3. `GOOGLE_DRIVE_PLANNING_SOURCES.md`
4. `TECH_STACK.md`
5. `PROJECT_STRUCTURE.md`
6. `CORE_HARNESS_IMPLEMENTATION_PROMPT.md`
7. `LINEAR_TASK_MAP.md`
8. `FIRST_DEMO_SPEC.md`

## Implementation Priority

Start with Linear issue:

```text
ELG-116 Create clean v0.2 workspace and core skeleton
```

Then continue through ELG-123.

## Agent Operating Model

Use the stable roles in `AGENT_ROSTER.md`.

Default implementation should use one focused agent at a time. Code Review Agent is available for risk gates, especially after approved filesystem mutation work and before expanding into TUI or live provider integration.

## Do Not Implement Yet

- full TUI
- LM Studio network provider
- Obsidian
- MCP
- Skills
- API
- Parallel Agents / Swarm
- Auto Skill Learning
- shell execution
- autonomous project generation

## Most Important Rule

Provider text is not truth.

Only the controller can record truth.
Only approved actions can change files.
Only the filesystem can confirm file results.
