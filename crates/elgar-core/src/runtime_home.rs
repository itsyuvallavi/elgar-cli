//! Elgar runtime home paths.
//!
//! This module owns user-level Elgar paths such as `~/.elgar/config` without
//! changing repo-local logs or session files.

use std::path::PathBuf;

pub const ELGAR_HOME_ENV: &str = "ELGAR_HOME";
pub const ELGAR_HOME_DIR: &str = ".elgar";
pub const CONFIG_DIR: &str = "config";

/// Returns the user-level Elgar home directory.
pub fn elgar_home_dir() -> PathBuf {
    std::env::var(ELGAR_HOME_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .map(|home| PathBuf::from(home).join(ELGAR_HOME_DIR))
        })
        .unwrap_or_else(|| PathBuf::from(ELGAR_HOME_DIR))
}

/// Returns the user-level config directory.
pub fn elgar_config_dir() -> PathBuf {
    elgar_home_dir().join(CONFIG_DIR)
}

/// Returns a named user-level config file path.
pub fn global_config_file(file_name: &str) -> PathBuf {
    elgar_config_dir().join(file_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn elgar_home_defaults_under_home() {
        let _guard = env_lock();
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let previous = std::env::var_os(ELGAR_HOME_ENV);

        std::env::remove_var(ELGAR_HOME_ENV);

        assert_eq!(elgar_home_dir(), PathBuf::from(home).join(ELGAR_HOME_DIR));

        match previous {
            Some(value) => std::env::set_var(ELGAR_HOME_ENV, value),
            None => std::env::remove_var(ELGAR_HOME_ENV),
        }
    }

    #[test]
    fn elgar_home_env_overrides_home() {
        let _guard = env_lock();
        let root =
            std::env::temp_dir().join(format!("elgar-core-runtime-home-{}", std::process::id()));
        let previous = std::env::var_os(ELGAR_HOME_ENV);
        std::env::set_var(ELGAR_HOME_ENV, &root);

        assert_eq!(elgar_home_dir(), root);

        match previous {
            Some(value) => std::env::set_var(ELGAR_HOME_ENV, value),
            None => std::env::remove_var(ELGAR_HOME_ENV),
        }
    }
}
