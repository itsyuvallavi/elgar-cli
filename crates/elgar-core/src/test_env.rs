use std::{
    ffi::OsString,
    path::Path,
    sync::{Mutex, MutexGuard},
};

static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct EnvGuard {
    previous_home: Option<OsString>,
    _home_lock: MutexGuard<'static, ()>,
}

impl EnvGuard {
    pub(crate) fn set_home(value: &Path) -> Self {
        let home_lock = HOME_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous_home = std::env::var_os("HOME");
        std::env::set_var("HOME", value);
        Self {
            previous_home,
            _home_lock: home_lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous_home) = &self.previous_home {
            std::env::set_var("HOME", previous_home);
        } else {
            std::env::remove_var("HOME");
        }
    }
}
