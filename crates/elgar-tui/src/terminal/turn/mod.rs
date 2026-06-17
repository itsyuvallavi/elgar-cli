//! Terminal provider-turn modules.
//!
//! This folder owns submitted prompt handling, active provider request behavior,
//! and background provider worker tasks.

pub(super) mod active;
pub(super) mod finalize;
pub(super) mod provider;
pub(super) mod provider_logging;
pub(super) mod provider_watchdog;
pub(super) mod provider_worker;
pub(super) mod submitted;
