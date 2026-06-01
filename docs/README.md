# docs

## Purpose

Active project documentation for Elgar v0.10 architecture, local checks,
provider behavior, permission policy, and TUI direction.

## Start Here

- `elgar-product-architecture-plan.md` is the current product/runtime contract.
- `codex-style-agent-runtime-plan.md` is the current migration reference.
- `local-checks.md` documents no-network verification commands.
- `live-provider-smoke.md` documents optional LM Studio smoke commands.
- `live-tui-file-planning-regression-checklist.md` documents manual live TUI
  file-planning regression prompts and pass criteria.

## Operational References

- `permissioned-actions-review.md` documents action-gate and executor safety.
- `permissioned-shell-commands.md` documents shell execution boundaries.
- `provider-compatibility.md` documents optional provider metadata.
- `performance-baselines.md` documents local timing baselines.
- `read-only-memory-context.md` documents the current read-only memory source.
- `tui-visual-qa-checklist.md` documents manual TUI visual checks.

## TUI Direction

- `pi-like-tui-direction.md` defines interaction tone and boundaries.
- `pi-like-terminal-tui-visual-spec.md` defines terminal rendering direction.

## Planning Exports

`planning/` is reserved for exported planning docs when available. Linear is the
execution map for current implementation work.

## Ownership

Keep docs aligned with implemented behavior. Delete or merge historical plans
when they start competing with the active architecture contract.

## Checks

- `./bin/check-local`
- `git diff --check`
- Review changed docs for stale command names, paths, and controller-first
  normal-chat language.
