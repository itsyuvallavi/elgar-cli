//! End-to-end smoke tests for the compiled `elgar` binary.
//!
//! This file is the integration-test entry point. Focused test groups live in
//! `tests/smoke/` so CLI runtime, provider config, scripted TUI, and permission
//! smoke coverage stay readable.

#[path = "smoke/cli_runtime_test.rs"]
mod cli_runtime_test;
#[path = "smoke/permissions_test.rs"]
mod permissions_test;
#[path = "smoke/provider_config_test.rs"]
mod provider_config_test;
#[path = "smoke/support.rs"]
mod support;
#[path = "smoke/tui_script_test.rs"]
mod tui_script_test;
