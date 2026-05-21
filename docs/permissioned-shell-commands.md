# Permissioned Shell Commands

ELG-218 executes shell commands only after explicit controller approval.

## Action Model

A `ShellCommand` proposal records:

- command text as opaque shell input
- cwd from the active session
- timeout in seconds
- expected effect text for approval review
- risk notes for approval review
- stdout and stderr output caps
- environment policy

Default policy:

- timeout: 30 seconds
- stdout cap: 16 KiB
- stderr cap: 16 KiB
- environment: inherit the controller process environment

## Routing

Only explicit shell command prefixes create a proposed shell action:

- `run ...`
- `run command ...`
- `run shell ...`
- `run shell command ...`
- `shell command ...`

Questions such as `can you run ...?` still go to the provider as model text.
Bare shell syntax such as `bash -lc ...` is not a command proposal by itself.

## Execution Boundary

Proposed shell commands do not execute.
Rejected shell commands do not execute.
Approved shell commands execute once through the controller-owned shell executor.

Provider text never executes shell and never proves command truth.

Successful executor completion records `VerifiedActionResult::Shell` with:

- command
- cwd
- capped stdout and stderr
- stdout/stderr truncation flags
- exit code when available
- elapsed milliseconds
- timeout status

Nonzero exit and timeout are shell-owned results, not provider truth.
