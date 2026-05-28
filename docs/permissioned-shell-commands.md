# Permissioned Shell Commands

ELG-218 introduced shell commands behind explicit approval. Current
AgentRuntime work keeps shell execution policy-owned: shell commands may run
only when the selected permission mode allows them or the user explicitly
approves them.

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
- maximum model-requested timeout: 300 seconds
- stdout cap: 16 KiB
- stderr cap: 16 KiB
- environment: inherit the Elgar process environment

## Routing

Normal user text goes to the model path. Elgar does not maintain natural
language shell prefixes that locally create shell actions.

The model may request the typed `shell_command` tool when the active
model-tool turn supports tools. Slash commands, such as `/tool`, are local UI
controls for choosing a runtime path; ordinary words are not local command
triggers.

Natural filesystem requests should prefer typed filesystem tools when possible.
Shell-backed filesystem work is reserved for cases that cannot be represented
as a safe typed file action under the active policy. User review is required in
`review_all`, `auto_create_review_modify`, and `workspace_write_with_review`.
`full_access` may apply a validated shell action immediately, and the executor
must verify any expected filesystem effect after the command finishes.

Questions such as `can you run ...?` still go to the provider as model text.
Bare shell syntax such as `bash -lc ...` is not a local command proposal by
itself.

## Execution Boundary

Proposed shell commands do not execute.
Rejected shell commands do not execute.
Approved or policy-allowed shell commands execute once through the shell
executor.

Provider text never executes shell and never proves command truth.

Timed-out shell commands and nonzero shell exits are failed actions. They are
not recorded as applied work.

Successful executor completion records `VerifiedActionResult::Shell` with:

- command
- cwd
- capped stdout and stderr
- stdout/stderr truncation flags
- exit code when available
- elapsed milliseconds
- timeout status
- expected filesystem effect when a shell-backed filesystem task can be verified

Shell status, output, and expected-effect verification are shell-owned results,
not provider truth.
