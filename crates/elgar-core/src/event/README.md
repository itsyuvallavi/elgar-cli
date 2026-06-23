# Event

This folder defines the facts Elgar records while it runs.

Events are used by:

- `session` to store the live conversation history
- `logs/sessions` to persist a JSONL copy
- TUI/CLI rendering to show what happened
- tests to inspect runtime behavior

Provider text is stored as provider output. It is not treated as proof that
files changed, commands ran, or any external action succeeded.
