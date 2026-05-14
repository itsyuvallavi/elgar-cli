# Linear Task Map

## Current Implementation Projects

### Elgar v0.2 — Core Harness

Goal: build the first trustworthy runtime center.

Milestones:

```text
M1: Workspace & Skeleton
M2: Router, Session & Events
M3: Controller & Provider Stub
M4: Permissioned WriteFile Proof
M5: No-Model Regression Gate
```

Issues:

```text
ELG-116 Create clean v0.2 workspace and core skeleton
ELG-117 Define core event types
ELG-118 Define router enum and basic routes
ELG-119 Define minimal session state
ELG-120 Implement controller turn flow with provider stub
ELG-121 Define action model and lifecycle
ELG-122 Implement approved WriteFile apply path
ELG-123 Add no-model Core Harness regression tests
```

### Elgar v0.2 — Simple TUI

Goal: create a minimal TUI over the Core Harness.

Issues:

```text
ELG-124 Create minimal TUI shell layout
ELG-125 Render Core Harness events in TUI
ELG-126 Add TUI action approval panel
ELG-127 Add first TUI smoke tests
```

### Elgar v0.2 — Harness & Regression Tests

Goal: protect the controller truth model and UI boundaries.

Issues:

```text
ELG-130 Add no-model router and controller tests
ELG-131 Add WriteFile action lifecycle regression tests
ELG-132 Add CLI/TUI boundary smoke tests
```

## Recommended Execution Order

1. ELG-116
2. ELG-117
3. ELG-118
4. ELG-119
5. ELG-120
6. ELG-121
7. ELG-122
8. ELG-123
9. ELG-130
10. ELG-131
11. ELG-124
12. ELG-125
13. ELG-126
14. ELG-127
15. ELG-132

## Recommended Agent Handoffs

```text
ELG-116 Core Harness Agent
ELG-117 Core Harness Agent
ELG-118 Router/Session Agent
ELG-119 Router/Session Agent
ELG-120 Core Harness Agent
ELG-121 Action Lifecycle Agent
ELG-122 Filesystem Safety Agent
ELG-122 review Code Review Agent
ELG-123 Harness/Test Agent
Core Harness review Code Review Agent
ELG-130 Harness/Test Agent
ELG-131 Harness/Test Agent
ELG-124 Simple TUI Agent
ELG-125 Simple TUI Agent
ELG-126 Simple TUI Agent
ELG-127 Simple TUI Agent
ELG-132 Harness/Test Agent
Provider / LM Studio work Provider / LM Studio Agent
```

Use one implementation agent at a time by default. Use Code Review Agent at risk gates rather than creating a separate review step for every small issue.

## Rule

Do not start TUI implementation until the Core Harness can propose/reject/approve/apply a WriteFile action and tests prove the lifecycle.
