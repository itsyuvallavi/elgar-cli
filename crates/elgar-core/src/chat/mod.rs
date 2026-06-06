//! Raw no-tool chat path.
//!
//! This is Elgar's smallest active provider path: user text goes to the model,
//! no tools are attached, and the model response is recorded into the session.

mod blocking;
mod streaming;

#[cfg(test)]
mod tests;

use crate::event::Event;

pub use blocking::run_raw_chat_turn;
pub use streaming::run_raw_chat_turn_streaming;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawChatTurnResult {
    pub events: Vec<Event>,
}
