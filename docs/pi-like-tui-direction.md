# Pi-like TUI Direction

Linear issue: ELG-143

## Decision

Elgar's TUI should be Pi-like in calmness, pacing, and chat-first focus, not in
identity, architecture, or feature set.

Pi is inspiration, not something to copy. Elgar's center remains:

```text
Controller owns truth.
Model suggests.
User approves.
Filesystem confirms.
UI reports.
```

The TUI should make that loop feel quiet and understandable instead of turning
it into a dashboard, log console, or autonomous agent surface.

## What Pi-like Means For Elgar

Pi-like means:

- conversational first, with the user's request and assistant response as the
  main surface
- calm status changes instead of noisy logs
- short, human-readable progress and error text
- minimal visible machinery until the user needs details
- local-model friendliness without making live provider access implicit
- a small core interaction that feels complete before adding advanced surfaces

The first TUI should feel like a focused conversation with visible trust checks,
not like an IDE, observability tool, or workflow dashboard.

## What Not To Copy

Elgar must not copy:

- Pi's identity, branding, or product voice
- any architecture that weakens controller truth
- hidden autonomy presented as smoothness
- broad extension/package concepts before the core is trusted
- prompt-only safety in place of typed controller state
- dashboard-heavy navigation, model managers, plugin browsers, or diagnostics
  pages in the first TUI

Smooth interaction is valuable only when it makes Elgar easier to trust.

## Tone And Interaction Principles

The TUI tone should be:

- direct
- calm
- concise
- specific about what happened
- explicit when approval is needed
- honest about provider errors and unverified claims

Avoid theatrical language, overexplaining internal mechanics, and developer-log
phrasing in the main conversation. Prefer messages like:

- `Thinking with local model...`
- `Proposed writing hello.py. Review before applying.`
- `Rejected. No file was changed.`
- `Applied and verified: hello.py was written.`
- `Provider error: model is not loaded.`

Do not say a file was written, command ran, or action succeeded unless the
controller recorded the verified result.

## Layout Direction

Keep the first layout small and stable:

- Conversation: primary region; shows user messages, assistant messages, compact
  provider progress, action summaries, verified results, and errors.
- Input: always available; one focused place to type a message or basic command.
- Status: one compact line for current state, provider/model when known, cwd or
  project root, pending action count, and last error if present.
- Approval panel: appears when a controller action is pending; shows action id,
  action type, target, short summary, and approve/reject affordance.

The approval panel should not become a side dashboard. It is a calm interruption:
the conversation can continue, but the user can clearly see what is waiting.

## Provider Progress And Errors

Provider progress should be visible but quiet:

- show start as a compact status or conversation line
- show finish by rendering the assistant/provider response
- show provider/model identity when known
- show errors in plain language with enough detail to act

Provider text is suggestion only. The TUI may show provider output, but it must
not convert provider claims into file, command, or action truth.

Default TUI/controller behavior remains no-network/stub. Live provider paths must
stay explicit.

## Permissioned Actions

Permissioned actions should feel calm but unmistakable:

- proposed actions are not failures or alarms
- the target and consequence must be visible
- approval and rejection must be deliberate
- rejection should feel final and safe
- applied results should report verified truth
- failed actions should explain what failed without implying partial success

The approval copy should make the boundary clear:

```text
Proposed WriteFile: hello.py
No file has been changed yet.
Approve to apply, or reject to leave the filesystem unchanged.
```

## Avoid Dashboard Drift

Do not add dashboard surfaces until the basic conversation/action loop is strong.
Defer:

- model manager
- settings pages
- memory browser
- plugin or skill browser
- advanced diagnostics
- token dashboards
- parallel agent panels
- broad command palettes

Diagnostics can exist later, but the main TUI should not make the user parse
internal logs to understand what happened.

## Keep Controller Truth Visible

Controller truth should be visible through small, stable signals:

- event-derived conversation lines
- compact status text
- one pending action panel
- verified result wording
- clear error wording

Do not expose every raw event by default. The TUI should summarize controller
truth without hiding it. A later diagnostics view can show raw event detail if
needed.

## Next Implementation Direction

First, split `crates/elgar-tui/src/shell.rs` into smaller modules before adding
more behavior.

After that split, the next TUI implementation issue should be:

```text
Add calm TUI copy and compact status rendering for controller events
```

Keep that issue small:

- refine user-facing text only where current event rendering is too log-like
- preserve existing controller events and tests
- do not add live provider mode
- do not add dashboards or diagnostics pages
- add focused smoke tests for conversation, status, provider error, and pending
  action wording
