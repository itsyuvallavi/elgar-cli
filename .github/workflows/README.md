# .github/workflows

## Purpose

GitHub Actions workflows for validating the Rust workspace.

## Important Files

- `ci.yml` runs local no-network checks and clippy on pushes and pull requests.

## Ownership

Prefer calling scripts from `bin/` so local and CI checks stay in sync.

## Checks

- `./bin/check-local`
- `cargo clippy --workspace --all-targets -- -D warnings`
