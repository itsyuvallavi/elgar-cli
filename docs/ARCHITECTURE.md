# Elgar Architecture

## Mental Model

```text
CLI = front door
TUI = terminal interface
Core = runtime engine
Provider = model connection
Logs = local audit trail
```

## Startup Flow

```text
elgar
-> crates/elgar-cli/src/main.rs
-> elgar-cli/src/startup/
-> elgar-tui
-> elgar-core chat/provider/session
```

`elgar-core` does not start the app. It is the reusable engine used by CLI,
TUI, tests, and future surfaces.

## Raw Chat Flow

```text
terminal input
-> TUI input handling
-> core chat turn
-> provider request with no tools
-> provider output
-> session events
-> TUI renders visible text
```

Current plain chat must not attach tools, inject memory, run folder anchoring,
or make a second synthesis request.

## Crate Responsibilities

`elgar-cli`:

- parse process arguments
- find runtime paths
- load provider config
- launch TUI
- expose diagnostic commands

`elgar-tui`:

- read terminal input
- handle local slash commands
- render conversation state
- keep terminal-only UI state

`elgar-core`:

- provider types and requests
- chat turn execution
- sessions and events
- token/context accounting
- logs
- future runtime capabilities

## Current Slash Commands

Current raw-only local commands include:

```text
/raw <prompt>
/cancel
/clear
/new
/details last
/copy
/copy raw
/help
/commands
/exit
/quit
/q
```

Unknown slash commands are local errors. Plain text without `/` goes to the
model.

## Future Agent Path

Future tools should plug into core as typed capability layers. The model may
request typed tools, but the runtime must validate, policy must decide, and
executors must verify before the UI reports success.
