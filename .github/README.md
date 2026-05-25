# .github

## Purpose

GitHub repository automation for the active Elgar project.

## Important Folders

- `workflows/` contains CI workflow definitions.

## Ownership

Keep automation small and aligned with local scripts. CI should call checked-in commands instead of duplicating long command lists.

## Checks

- `./bin/check-local`
- `cargo clippy --workspace --all-targets -- -D warnings`
