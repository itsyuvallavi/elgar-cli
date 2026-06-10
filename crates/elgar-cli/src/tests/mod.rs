//! CLI unit tests split out of `lib.rs`.
//!
//! Keep these tests aligned with the active harness CLI. Tests for archived
//! tool/policy behavior belong in `_legacy`, not here.

mod local_command_test;
mod logs_test;
mod paths_test;
mod provider_config_test;
mod provider_smoke_test;
mod scripted_tui_test;
