# elgar-cli/src/tests

## Purpose

Focused unit tests for active CLI helper modules.

## Files

- `mod.rs` registers the CLI unit test modules.
- `paths_test.rs` checks project-root and cwd resolution.
- `provider_config_test.rs` checks `elgar-provider.json` loading.
- `provider_smoke_test.rs` checks direct LM Studio smoke-test config helpers.
- `scripted_tui_test.rs` checks line-based TUI slash commands and transcript behavior.

## Ownership

Keep these tests aligned with the active harness CLI. Do not test archived tool,
policy, or old agent-runtime behavior here.
