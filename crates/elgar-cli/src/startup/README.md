# elgar-cli/src/startup

## Purpose

Startup modules for the real `elgar` launch path.

## Files

- `mod.rs` registers and re-exports startup modules.
- `paths.rs` resolves project roots and repo-level provider config locations.
- `provider_config.rs` reads `elgar-provider.json`, environment overrides, and
  the user-level `~/.elgar/config/elgar-provider.json` fallback.
- `mcp_config.rs` loads MCP config from environment, repo-level config, or the
  user-level `~/.elgar/config/elgar-mcp.json` fallback.
- `terminal.rs` launches the interactive terminal TUI and passes startup MCP
  status into the visible startup block.

## Ownership

Keep startup focused on process/config decisions. Model behavior belongs in
`elgar-core`; terminal rendering belongs in `elgar-tui`.
