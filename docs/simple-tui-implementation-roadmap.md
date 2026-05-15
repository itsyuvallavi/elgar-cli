# Simple TUI Implementation Roadmap

Linear issue: ELG-152

## Purpose

This roadmap turns the current Simple TUI direction into small implementation
slices.

Elgar's TUI should become a usable terminal conversation surface while
preserving the core boundary:

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
Tests protect.
```

The TUI is not the runtime authority. It renders controller state, submits user
input, and offers clear approval affordances. It must not own file mutation,
action routing, provider selection, or truth about whether work succeeded.

## Current State

The repo already has:

- `TuiShell` rendering conversation, pending action, status, and input regions
- TUI smoke tests for controller events and action states
- calm copy and compact status rendering
- an explicit `tui-controller-smoke` live provider path
- a minimal line-oriented `cargo run -p elgar-cli -- tui` loop
- no-network default behavior through `Controller::default()`
- `./bin/check-local` as the normal no-network guardrail

The current loop is useful as a proving path, not the final terminal
experience. It prints a complete rendered shell after each submitted line.

## Target V0 Experience

V0 should feel like a calm local conversation with visible trust checks:

- the user opens `elgar tui` and sees a quiet startup surface
- the conversation is the primary surface
- the input area is always obvious and visually stable
- status is compact, human-readable, and anchored near the bottom
- proposed actions interrupt calmly and explicitly
- approval and rejection are deliberate
- applied results are reported only after controller/filesystem confirmation
- provider progress and errors are understandable without becoming logs
- default startup is no-network/stub

V0 is considered usable when a user can:

- ask a normal question or request
- see controller-derived assistant output
- request a file action
- review the pending action
- approve or reject it from the TUI
- see verified apply/reject results
- exit cleanly
- run the default path without any provider network call

## Non-Goals For V0

Do not add these before the basic loop is strong:

- default live provider mode
- streaming
- JSON config
- multi-provider routing
- model manager
- settings pages
- dashboard panels
- diagnostics console
- plugin or skill browser
- parallel agent views
- autonomous file mutation without approval

Pi is inspiration for calmness and pacing only. Do not copy Pi's identity,
branding, product voice, or hidden autonomy.

## Reference Lessons From Pi Screenshots

The shared Pi screenshots are useful as interaction references, not as a
product to copy. Keep the lessons that fit Elgar's boundaries:

- Startup should be quiet and informational.
  - show loaded local context in a small area, such as `AGENTS.md`
  - show available local capabilities only if they are real and relevant
  - avoid a large welcome screen or marketing copy

- Input should feel like the center of action.
  - use a stable input band near the lower part of the terminal
  - keep a visible cursor and clear submit behavior
  - avoid reprinting the whole shell after each turn once the real TUI exists

- Progress should be calm and short.
  - a single line like `Working...` is enough while a turn runs
  - provider/model details belong in the footer or compact status, not as logs

- Slash commands should open a focused command palette.
  - show command names and one-line descriptions
  - keep commands scoped to local TUI affordances and controller APIs
  - do not expose a broad settings dashboard in V0

- Selectors should be bounded and contextual.
  - model/provider selection stays deferred until explicit provider mode exists
  - settings-like selectors can inspire later UI patterns, but not V0 scope

- The footer should carry stable environment context.
  - cwd or project root
  - git branch when cheap and local
  - model/provider only when explicitly active
  - token/context budget only after the controller tracks it

- Rich assistant output matters.
  - markdown tables, code blocks, and tree output should render legibly
  - long answers should scroll naturally
  - raw markdown artifacts like `<br>` should not leak into the final TUI if
    avoidable

- Color should carry hierarchy, not noise.
  - use one accent for user input and selected items
  - use muted text for hints and startup context
  - use stronger contrast for verified results and pending approvals

Do not copy:

- Pi's exact command set
- provider/model settings behavior
- token accounting unless Elgar has its own controller-backed data
- login/provider onboarding
- raw broad settings panels before Elgar needs them

## Phase 1: Finish The Line-Oriented Loop

Keep the current stdout/stdin loop and make it behaviorally complete before
switching terminal frameworks.

Implementation slices:

1. Add interactive approval commands.
   - `/approve` submits approval through `TuiShell::submit_approval`.
   - `/reject` submits rejection through `TuiShell::submit_rejection`.
   - commands are local TUI affordances, not new controller truth.
   - normal free text still goes through `Controller::turn`.

2. Add pending-action command feedback.
   - `/pending` renders the current pending action area.
   - approving with no pending action should produce calm local feedback.
   - rejecting with no pending action should produce calm local feedback.

3. Add a small local command list.
   - `/help` or `/commands` shows only supported local TUI commands.
   - descriptions should be one line each.
   - do not add model/settings commands yet.

4. Add line-loop smoke tests for action lifecycle.
   - create a WriteFile proposal
   - prove the file is not written before approval
   - reject and prove no mutation
   - create a new proposal and approve
   - prove verified result is rendered

Acceptance gate:

- default `cargo run -p elgar-cli -- tui` remains no-network
- action lifecycle goes through controller APIs
- `./bin/check-local` passes

## Phase 2: Introduce A Real Terminal Shell

Move from repeated stdout renders to an interactive terminal app only after
Phase 1 is covered by tests.

Use:

```text
ratatui
crossterm
```

Implementation slices:

1. Add a minimal app loop module.
   - owns terminal setup, teardown, key input, and redraw
   - delegates all controller work to existing TUI/controller helpers
   - keeps current line loop available until the terminal loop is stable

2. Render the same four regions.
   - conversation
   - input
   - compact status
   - pending action panel

3. Add Pi-inspired terminal layout details.
   - conversation scrollback fills the main area
   - input is a stable band near the bottom
   - footer shows cwd/project and explicit provider/model state
   - transient command palettes open above the footer

4. Add basic keyboard behavior.
   - type/edit a single-line input
   - Enter submits
   - Esc or Ctrl-C exits cleanly
   - mapped approval keys only when an action is pending

5. Add resize-safe layout tests or snapshots where practical.
   - do not chase pixel-perfect styling yet
   - protect region presence and text clarity

Acceptance gate:

- terminal setup always restores the terminal on exit/error
- controller truth still drives rendered state
- no live provider path is introduced
- line-loop tests remain green
- the input/footer region stays stable across normal redraws

## Phase 3: Make Approval Feel Native

Once the real terminal shell exists, make permissioned actions obvious without
turning the UI into a dashboard.

Implementation slices:

1. Improve pending action focus.
   - show action type, target, summary, and consequence
   - show `No file has been changed yet`
   - show approve/reject affordances only when relevant

2. Add deliberate approval controls.
   - approval should require a clear key or command
   - rejection should be equally easy and final
   - a rejected action must never later mutate files

3. Add verified result presentation.
   - successful apply: `Applied and verified: <target>`
   - rejection: `Rejected. No file was changed.`
   - failure: explain what failed without implying partial success

Acceptance gate:

- approval/rejection cannot bypass controller action state
- verified result wording is backed by controller events
- tests cover approved, rejected, failed, and no-pending cases

## Phase 4: Provider Progress And Errors

Provider work remains explicit until the product has a shared provider
configuration decision. This phase is about presentation first, not live default
mode.

Implementation slices:

1. Render provider lifecycle quietly.
   - start: `Thinking with <provider>...`
   - finish: render assistant response
   - error: `Provider error: <actionable message>`

2. Add explicit live TUI launch only if config boundaries are ready.
   - likely behind a named command or flag
   - never activated by LM Studio env vars alone
   - must have tests proving default remains stub/no-network

3. Add retry guidance for provider errors.
   - keep it short
   - do not turn the main UI into diagnostics

Acceptance gate:

- normal tests make no provider network calls
- default TUI stays stub/no-network
- provider text is never treated as file/action truth

## Phase 5: Conversation Usability

Add comfort features after the controller/action loop is trustworthy.

Implementation slices:

1. Scrollback.
   - conversation remains primary
   - raw event detail stays hidden by default
   - long assistant output can be read without losing the input area

2. Rich text rendering.
   - render markdown paragraphs, lists, code blocks, tables, and file trees
   - preserve readable wrapping in narrow terminals
   - avoid leaking provider markdown artifacts when the renderer can normalize
     them

3. Input history.
   - previous submitted prompts
   - no persistence until there is a session persistence decision

4. Multiline input if needed.
   - only if real workflows need it
   - preserve clear submit behavior

5. Session naming and resume decision.
   - define storage and truth boundaries before implementation
   - avoid hidden persistence

Acceptance gate:

- usability features do not change controller semantics
- no file/action truth is inferred from UI state

## Suggested Linear Issue Sequence

Create these as small follow-up issues:

```text
Add line-oriented TUI approval commands
Add local TUI command help for the line loop
Add TUI loop action lifecycle smoke tests
Add minimal ratatui/crossterm terminal app shell
Render TuiShell regions in terminal layout
Add terminal input handling and clean exit
Add native approval controls to terminal TUI
Add provider progress/error presentation tests for TUI
Add markdown rendering for TUI assistant output
Add conversation scrollback for terminal TUI
```

Do not start provider streaming until the explicit provider progress/error phase
is complete and tested.

## Next Recommended Issue

Start with:

```text
Add line-oriented TUI approval commands
```

That issue should add `/approve`, `/reject`, and focused tests around no-pending
and pending-action behavior in the existing `cargo run -p elgar-cli -- tui`
path. It should not add `ratatui`, live provider mode, streaming, or new action
types.
