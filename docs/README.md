# docs

## Purpose

Active project documentation for Elgar v0.2 design, checks, provider compatibility, TUI direction, and planning exports.

## Important Files and Folders

- `local-checks.md` documents local verification commands.
- `permissioned-actions-review.md` and `permissioned-shell-commands.md` document safety boundaries.
- `provider-compatibility.md` and `live-provider-smoke.md` document provider behavior.
- `planning/` contains exported planning docs when available.

## Ownership

Keep docs aligned with implemented behavior. Planning docs are references, not a substitute for tests.

## Checks

- `./bin/check-local`
- Review changed docs for stale command names and paths.
