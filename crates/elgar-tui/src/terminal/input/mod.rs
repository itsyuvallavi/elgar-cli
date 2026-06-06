//! Terminal input modules.
//!
//! This folder owns keyboard mapping, paste cleanup, and terminal raw-mode
//! prompt reading. Terminal raw mode is not raw chat.

pub(super) mod keymap;
pub(super) mod normalization;
pub(super) mod raw_mode;
pub(super) mod read;
