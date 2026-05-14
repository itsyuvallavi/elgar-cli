# Core Harness Implementation Prompt

Use this prompt with Codex or another coding agent.

## Task

Implement the first Elgar v0.2 Core Harness slice.

## Source of Truth

Read the planning docs first.

Canonical Google Drive planning index:

```text
ELGAR_PLANNING_INDEX
https://docs.google.com/document/d/1-V7QT5Au67g20pR5OAzh2_LpAZxIX0NLiUXsl2TW66c/edit
```

Exact source list:

```text
GOOGLE_DRIVE_PLANNING_SOURCES.md
```

Canonical source is the Google Drive planning folder/index. If the repo already has exported Markdown copies, use:

```text
docs/planning/ELGAR_V0_2_PLAN.md
docs/planning/PRODUCT_PRINCIPLES.md
docs/planning/CONTROLLER_TRUTH_MODEL.md
docs/planning/RESPONSIBILITY_BOUNDARIES.md
docs/planning/CORE_HARNESS_ROADMAP.md
docs/planning/PERMISSIONED_ACTIONS_ROADMAP.md
docs/planning/HARNESS_REGRESSION_TESTS_ROADMAP.md
```

If those files are absent, do not continue as if they exist. Create `docs/planning/` and add/export the required planning docs first, or report that implementation is blocked on missing planning docs.

## Current Linear Project

```text
Elgar v0.2 — Core Harness
```

## Agent Roster

Use `AGENT_ROSTER.md` for future handoffs.

Core Harness work should use the smallest matching stable role:

```text
Core Harness Agent
Router/Session Agent
Action Lifecycle Agent
Filesystem Safety Agent
Harness/Test Agent
Code Review Agent
```

Use Code Review Agent at risk gates, especially after approved filesystem mutation work and before declaring the Core Harness slice complete.

Start with:

```text
ELG-116 Create clean v0.2 workspace and core skeleton
```

Then continue in order:

```text
ELG-117 Define core event types
ELG-118 Define router enum and basic routes
ELG-119 Define minimal session state
ELG-120 Implement controller turn flow with provider stub
ELG-121 Define action model and lifecycle
ELG-122 Implement approved WriteFile apply path
ELG-123 Add no-model Core Harness regression tests
```

## Required Behavior

Build a tiny runtime that can:

1. accept user input,
2. route input,
3. call a provider stub for AskModel,
4. propose a WriteFile action,
5. reject a proposed action without writing,
6. approve a proposed action,
7. apply an approved WriteFile action,
8. verify the file exists after writing,
9. report the result through events/renderer,
10. pass no-model tests.

## Hard Boundaries

Do not implement:

- full TUI
- LM Studio network call
- Obsidian integration
- MCP
- Skills
- API
- Parallel Agents
- Auto Skill Learning
- shell execution
- autonomous coding loop

## Core Types to Create

### Event

Examples:

```text
UserMessage
AssistantMessage
ProviderStarted
ProviderFinished
ActionProposed
ActionApproved
ActionRejected
ActionApplied
ActionFailed
Error
```

### Route

Examples:

```text
AskModel
ProposeWriteFile
ApproveAction
RejectAction
Help
Unknown
```

### Action

Examples:

```text
WriteFile
```

States:

```text
Proposed
Approved
Applied
Rejected
Failed
```

### Session

Store:

```text
session id
project root / cwd
events
actions
provider metadata
```

## First Tests

Required:

- provider response does not mutate files
- router classifies AskModel input
- router classifies create-file request
- unknown input is safe
- proposed WriteFile does not create file
- rejected WriteFile does nothing
- approved WriteFile creates exactly target file
- controller records action states
- renderer reports proposed/rejected/applied states

## Report Back

When done, report:

- files created
- modules implemented
- tests added
- commands run
- test results
- known limitations
- next recommended Linear issue
