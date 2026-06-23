//! Local context loading and prompt assembly.
//!
//! Context here means optional text Elgar may attach to a model request:
//! local instruction files, provider config, and bounded memory notes. This is
//! related to the model context window, not the TUI screen and not model
//! reasoning.

mod accounting;
mod budget;
mod bundle;
mod loading;

pub use accounting::{ContextAccounting, LoadedContextFile, OmittedContextFile};
pub use budget::{context_budget_tokens, DEFAULT_CONTEXT_BUDGET_TOKENS};
pub use bundle::ContextBundle;
pub use loading::{DEFAULT_CONTEXT_FILES, LOCAL_MEMORY_DIR, LOCAL_MEMORY_FILE_LIMIT};
