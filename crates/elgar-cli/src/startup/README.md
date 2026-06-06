# elgar-cli/src/startup

## Purpose

Startup modules for the real `elgar` launch path.

## Files

- `mod.rs` registers and re-exports startup modules.
- `paths.rs` resolves project roots and provider config locations.
- `provider_config.rs` reads `elgar-provider.json` and environment overrides.
- `terminal.rs` launches the interactive terminal TUI.

## Ownership

Keep startup focused on process/config decisions. Model behavior belongs in
`elgar-core`; terminal rendering belongs in `elgar-tui`.
