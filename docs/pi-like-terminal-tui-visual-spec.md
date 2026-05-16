# Pi-like Terminal TUI Visual Spec

Linear issue: ELG-173

## Decision

Elgar's terminal TUI should be Pi-like, but simpler.

The target is a borderless terminal chat that feels calm, local, and truthful.
It should not become a boxed dashboard, model console, settings app, or fake
agent platform.

Before committing TUI polish, use `docs/tui-visual-qa-checklist.md`.

The controller boundary remains the product boundary:

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
```

## Screen Shape

Use a borderless vertical layout:

```text
Startup
Commands
Context
Provider

create hello.py

thinking...

Model:
I can propose that file. Review the action before applying.

Action review
Proposed WriteFile: hello.py
No file has been changed yet.
/approve to apply, /reject to leave the filesystem unchanged.

> |

~/__git/elgar (<branch>)                         <model>
context: TBD
```

Avoid panel borders around normal chat. Use whitespace, short labels, and muted
color for hierarchy.

## Startup Block

Startup should be short and factual:

```text
Elgar
Local controller TUI

Commands
/commands  Show commands
/approve   Apply the pending action
/reject    Reject the pending action
/exit      Quit

Context
AGENTS.md
elgar-provider.json

Provider
<configured provider>/<configured model>
```

Rules:

- show only real local files that were actually loaded or selected as context
- show provider/model only when configured or active
- do not show Skills, MCP, tools, providers, or capabilities that do not exist
- keep commands slash-only; do not add natural-language command chips

## Chat Blocks

Use simple message blocks with minimal labels:

- Submitted user messages render as a full-width muted block, without a `User`
  label and without a visible `>` prompt marker in the transcript area.
- Transient work renders as muted `thinking`, `thinking.`, `thinking..`, or
  `thinking...` inside the chat area.
- Final provider or controller responses keep the light `Model:` label.
- `Action review` for pending permissioned actions

Thinking should be muted and short:

```text
thinking...
```

Do not stream verbose internal logs into the main chat. Errors should be plain
and specific:

```text
Provider error: model is not loaded.
```

## Action Review

Permissioned actions must be visually clear without becoming modal dashboards:

```text
Action review
Proposed WriteFile: hello.py
No file has been changed yet.
/approve to apply, /reject to leave the filesystem unchanged.
```

The TUI must never imply an action succeeded until controller/filesystem truth
confirms it.

## Footer

The footer is a compact environment line:

```text
~/__git/elgar (<branch>)                         openai/gpt-oss-20b
context: TBD
```

Rules:

- first line left side: folder/repo and branch when known
- first line right side: model when known, otherwise the provider if no model is known
- second line: `context: TBD` until real controller-backed accounting exists
- `context: TBD` is acceptable until controller-backed context accounting exists
- do not fake `128k`, token counts, percentages, context budget bars, or max
  window size
- omit unknown values or mark them plainly as `TBD`
- do not show helper copy such as native selection, PgUp/PgDn, or `/copy` in
  the footer

## Native Terminal Behavior

Preserve native terminal scrolling and text selection. The TUI should not enable
mouse capture or alternate behavior that prevents selecting visible text with
the terminal emulator.

Long responses should rely on terminal-native scrollback first. In-app scrolling
can be added later only if it does not break normal selection.

## What Not To Copy From Pi

Do not copy:

- Pi branding, naming, identity, or product voice
- boxed dashboard layouts
- fake Skills, MCP, tools, memory, or capability sections
- provider onboarding or login flows
- broad model/settings panels
- token windows, percentages, or budget gauges not backed by Elgar data
- hidden autonomy presented as smoothness
- any UI claim that bypasses controller truth

Pi is an interaction reference for calmness and pacing only.
