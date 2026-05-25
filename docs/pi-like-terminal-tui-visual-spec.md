# Pi-like Terminal TUI Visual Spec

Linear issue: ELG-173

## Decision

Elgar's terminal TUI should be Pi-like, but simpler.

The target is a borderless terminal chat that feels calm, local, and truthful.
It should not become a boxed dashboard, model console, settings app, or fake
agent platform.

Before committing TUI polish, use `docs/tui-visual-qa-checklist.md`.

The runtime boundary is the product boundary:

```text
Model owns intent.
Runtime validates.
Policy decides.
Executors verify.
UI reports.
Tests protect.
```

## Screen Shape

Use an inline terminal layout, not a full-screen redraw. Startup prints once.
While editing, Elgar redraws only a small prompt/footer frame. After submit, the
prompt frame is cleared and the conversation remains in normal terminal
scrollback.

The live shape should look like:

```text
Startup
Commands
Context
Provider

create hello.py

thinking...

I can propose that file. Review the action before applying.

Action review
Proposed WriteFile: hello.py
No file has been changed yet.
/approve to apply, /reject to leave the filesystem unchanged.

▸ |
────────────────────────────────────────────────────────────

~/__git/elgar (<branch>)                         <model>
context: TBD
```

Avoid full-screen panel layouts around normal chat. Use whitespace, short labels,
and muted color for hierarchy.

## Startup Block

Startup should be short and factual:

```text
Elgar
Local AgentRuntime TUI

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
- Final provider responses render without a visible `Model:` label; runtime
  messages may keep a light `Elgar:` label when the distinction matters.
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

The TUI must never imply an action succeeded until runtime/executor truth
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
- second line: `context: TBD` until real runtime-backed accounting exists
- `context: TBD` is acceptable until runtime-backed context accounting exists
- do not fake `128k`, token counts, percentages, context budget bars, or max
  window size
- omit unknown values or mark them plainly as `TBD`
- do not show helper copy such as native selection, PgUp/PgDn, or `/copy` in
  the footer

## Native Terminal Behavior

Preserve native terminal scrolling and text selection. The TUI should not enable
mouse capture or alternate behavior that prevents selecting visible text with
the terminal emulator.

Long responses should rely on terminal-native scrollback first. The runtime
should not keep a large blank full-screen region just to anchor a footer at the
bottom.

## What Not To Copy From Pi

Do not copy:

- Pi branding, naming, identity, or product voice
- boxed dashboard layouts
- fake Skills, MCP, tools, memory, or capability sections
- provider onboarding or login flows
- broad model/settings panels
- token windows, percentages, or budget gauges not backed by Elgar data
- hidden autonomy presented as smoothness
- any UI claim that bypasses runtime validation or executor verification

Pi is an interaction reference for calmness and pacing only.
