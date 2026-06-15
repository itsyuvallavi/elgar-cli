//! Cooperative cancellation for active provider requests.
//!
//! The TUI owns user intent, but provider transports need a small shared token
//! so `/cancel` can stop an active socket instead of only hiding late results.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use super::ProviderError;

/// Shared cancellation token checked by harness and provider transports.
#[derive(Debug, Clone, Default)]
pub struct ProviderCancelToken {
    canceled: Arc<AtomicBool>,
}

impl ProviderCancelToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.canceled.store(true, Ordering::SeqCst);
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::SeqCst)
    }

    pub fn error_if_canceled(&self) -> Result<(), ProviderError> {
        if self.is_canceled() {
            Err(ProviderError::canceled("provider request canceled"))
        } else {
            Ok(())
        }
    }
}
