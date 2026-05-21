# Read-Only Local Memory Context

ELG-225 adds a small local memory source to controller-owned context selection.

## Convention

Elgar reads Markdown notes from:

```text
.elgar/memory/*.md
```

Only direct child `.md` files are considered. Files are sorted by filename and
capped at eight notes per turn. Missing directories, unreadable directories, and
non-Markdown files are ignored.

## Boundary

Memory notes are prompt context only. They cannot:

- approve actions,
- execute shell commands,
- mutate files,
- verify filesystem truth,
- override controller policy.

The controller still owns truth. The model may use memory notes to suggest a
response or a proposed action, but approval and verification remain unchanged.

## Budgeting

Memory notes share the existing local context budget with `AGENTS.md` and
`elgar-provider.json`. Included, trimmed, and omitted memory files are reported
through the same context accounting structures as other local context files.

## Deferred

This is not Obsidian integration. MCP, Skills, Obsidian APIs, backlinks, search,
and permissioned note writes stay deferred until read-only local context proves
useful.
