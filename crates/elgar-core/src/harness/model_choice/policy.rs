//! Model-choice protocol policy constants.
//!
//! These limits are runtime safety policy, not natural-language intent rules.

/// Maximum native or JSON fallback primitive requests accepted in one provider
/// response.
pub const MAX_TOOL_CALL_BATCH: usize = 8;
