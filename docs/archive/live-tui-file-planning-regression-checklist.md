# Live TUI File-Planning Regression Checklist

Use this checklist after harness, TUI, memory, route, or provider-prompt changes
to confirm the core file-planning path still works in the live line TUI.

This is a manual live-provider dogfood checklist. It is not part of
`./bin/check-local`.

## Setup

Start LM Studio with the intended model loaded, then run:

```sh
elgar tui
```

Record these before the run:

```text
date:
provider:
model:
cwd:
branch:
```

If the TUI is launched from inside `playground/`, omit the `playground/` prefix
from the prompts below. Creating `playground/playground/...` from that cwd is a
known path-anchoring quirk, not a failure by itself.

Do not run shell-verification prompts in this checklist. The point is to verify
file planning, file creation, verified state, memory, and TUI observability.

## Pass Criteria

- Plain `hello` remains a normal chat reply.
- Plan-only prompts create only the plan file first.
- Plan readback shows expected present/missing files from verified state.
- Follow-up execution creates all expected files.
- Same-prompt plan-plus-execute creates the plan and implementation files.
- Created-file questions use verified state and include the expected files.
- Second-project questions can focus on only the requested project.
- "First file" questions report the first verified artifact from the session.
- The `Observability` section appears after normal turns and shows route,
  decision, selected memory, plan status, provider request summary, and context.

## Prompt Set

Paste these one at a time.

### 1. Same-Prompt Plan And Execute

```text
Create a complete small local Python bookmark manager project. The project root must be exactly playground/LiveChecklistBookmark1. First create a project plan at playground/LiveChecklistBookmark1/PLAN.md, then execute that same plan. Do not create any files outside playground/LiveChecklistBookmark1. Include exactly these implementation files: README.md, requirements.txt, src/main.py, tests/test_main.py. Include verification and acceptance criteria in the plan. Keep it minimal but runnable. Do not run shell commands. If the plan mentions a database or data file, mark it as generated at runtime, not as a file to create.
```

```text
What files did you create in playground/LiveChecklistBookmark1?
```

Expected:

```text
project: playground/LiveChecklistBookmark1
status: completed
files: 4/4 present
- ok playground/LiveChecklistBookmark1/README.md
- ok playground/LiveChecklistBookmark1/requirements.txt
- ok playground/LiveChecklistBookmark1/src/main.py
- ok playground/LiveChecklistBookmark1/tests/test_main.py
```

### 2. Plan-Only, Interruption, Readback, Execute

```text
Create only a project plan for a tiny Python notes CLI. The project root must be exactly playground/LiveChecklistNotes1. Do not implement it yet. The plan must include README.md, requirements.txt, src/main.py, tests/test_main.py, verification, and acceptance criteria. Do not run shell commands.
```

```text
hello, quick interruption.
```

```text
Read the plan back to me with the expected files and folders, including which ones are present or missing.
```

Expected before execution:

```text
status: verified
directories: 1/3 present
files: 0/4 present
- missing playground/LiveChecklistNotes1/README.md
- missing playground/LiveChecklistNotes1/requirements.txt
- missing playground/LiveChecklistNotes1/src/main.py
- missing playground/LiveChecklistNotes1/tests/test_main.py
```

Then run:

```text
Please execute the plan you just created. Create all expected files and folders from the plan. Do not run shell commands.
```

```text
What files did you create in playground/LiveChecklistNotes1?
```

Expected after execution:

```text
project: playground/LiveChecklistNotes1
status: completed
files: 4/4 present
```

### 3. Unrelated Artifact Memory

```text
Create exactly one unrelated file named playground/LiveChecklistExtra1.txt with the text live checklist artifact memory check.
```

```text
What files have you created so far?
```

Expected:

- All plan and implementation files from `LiveChecklistBookmark1`.
- All plan and implementation files from `LiveChecklistNotes1`.
- `playground/LiveChecklistExtra1.txt`.

### 4. Second-Project Focus

```text
Create a second complete small project inside playground/LiveChecklistBudget1. First create a project plan, then execute it. It should be a tiny Python CLI for tracking a few expenses. Include README.md, requirements.txt, source code, tests, verification, and acceptance criteria. Keep it minimal but runnable. Do not run shell commands. If the plan mentions a database or data file, mark it as generated at runtime, not as a file to create.
```

```text
What files did you create in the second project? There should be only files from playground/LiveChecklistBudget1.
```

Expected:

- The answer is scoped to `playground/LiveChecklistBudget1`.
- It does not list files from `LiveChecklistBookmark1`, `LiveChecklistNotes1`,
  or the unrelated extra file.
- It reports all expected files from the second project's verified plan.

### 5. Earliest Artifact

```text
What was the first file you created in this whole session?
```

Expected:

```text
first created: playground/LiveChecklistBookmark1/PLAN.md
```

The exact action id and turn number may vary.

## Failure Triage

If a step fails, collect:

```text
/reasoning
/memory
/plan
/tokens
/status
```

Also check local traces when available:

```sh
find .elgar/traces -type f -name '*.jsonl' -print
tail -n 200 .elgar/traces/*.jsonl
```

Classify the failure before changing code:

- **Model failure**: model ignores clear verified state even though memory and
  plan previews are correct.
- **Harness memory failure**: `/memory` or Observability misses verified plan or
  artifact facts that should exist.
- **Plan guard failure**: off-plan files are applied, or expected files are not
  created during explicit plan execution.
- **UI reporting failure**: files are correct on disk, but TUI state output is
  misleading.
- **Provider/runtime failure**: network timeout, malformed provider JSON, or
  repeated nonsense output from the model.

Do not treat `playground/playground/...` as a failure when the TUI was launched
from inside `playground/`; record the cwd and rerun from repo root if path
anchoring itself is under investigation.
