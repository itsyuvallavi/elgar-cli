# First Demo Spec

## Goal

Define the first useful Elgar v0.2 demo.

This is not a full product demo. It is a proof that the core loop is trustworthy.

## Demo Flow

```text
1. Start Elgar CLI.
2. User asks: create hello.py that prints hello
3. Elgar proposes a WriteFile action.
4. User rejects.
5. Elgar reports rejected and writes nothing.
6. User asks again, creating a new WriteFile proposal.
7. User approves the new proposal.
8. Elgar writes hello.py.
9. Elgar verifies the file exists.
10. Elgar reports the verified write.
```

## Rejected Action Rule

Rejected actions are terminal.

A rejected action must never later mutate the filesystem. If the user changes their mind, Elgar should create a new proposal and require approval for that new action.

## Required Truth Claims

Elgar may say:

```text
I propose writing hello.py.
```

before approval.

Elgar may say:

```text
hello.py was written.
```

only after the controller applied the action and verified the filesystem result.

## Required Test Proof

The demo is not valid unless tests prove:

- proposed write does not create a file
- rejected write does nothing
- approved write creates the file
- provider text cannot create the file
- renderer reports action state correctly

## Non-Goals

Do not include:

- full TUI
- LM Studio live provider
- shell commands
- Obsidian
- MCP
- Skills
- API
- parallel agents
- auto skill learning

## Success Criteria

The demo succeeds when it is boringly reliable.

The user can see what Elgar wants to do, approve it, and trust the result.
