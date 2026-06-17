//! Provider-turn watchdog settings for interactive terminal turns.
//!
//! The watchdog is a UI safety net: if an interactive request never completes,
//! the TUI cancels it and reports a user-safe message.

use std::time::Duration;

const DEFAULT_INTERACTIVE_PROVIDER_WATCHDOG_MILLIS: u64 = 300_000;
const PROVIDER_WATCHDOG_ENV: &str = "ELGAR_TUI_PROVIDER_WATCHDOG_MILLIS";

pub(super) fn interactive_provider_watchdog_timeout() -> Duration {
    watchdog_timeout_from_env_value(std::env::var(PROVIDER_WATCHDOG_ENV).ok().as_deref())
}

fn watchdog_timeout_from_env_value(value: Option<&str>) -> Duration {
    value
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|millis| *millis > 0)
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(DEFAULT_INTERACTIVE_PROVIDER_WATCHDOG_MILLIS))
}

pub(super) fn provider_watchdog_timeout_message() -> &'static str {
    "Model response took too long and was canceled. No changes were applied. Try a shorter prompt or retry."
}

#[cfg(test)]
mod tests {
    use super::{provider_watchdog_timeout_message, watchdog_timeout_from_env_value};

    #[test]
    fn watchdog_timeout_uses_default_without_env_value() {
        assert_eq!(watchdog_timeout_from_env_value(None).as_millis(), 300_000);
        assert_eq!(
            watchdog_timeout_from_env_value(Some("0")).as_millis(),
            300_000
        );
        assert_eq!(
            watchdog_timeout_from_env_value(Some("bad")).as_millis(),
            300_000
        );
    }

    #[test]
    fn watchdog_timeout_accepts_positive_env_millis() {
        assert_eq!(
            watchdog_timeout_from_env_value(Some("1500")).as_millis(),
            1_500
        );
    }

    #[test]
    fn watchdog_timeout_message_is_user_safe() {
        let message = provider_watchdog_timeout_message();

        assert!(message.contains("canceled"));
        assert!(message.contains("No changes were applied"));
    }
}
