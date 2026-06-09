//! Provider request-mode names for harness model calls.
//!
//! The harness decides what kind of provider call it is making, then the
//! provider config decides which backend serves that request mode. This keeps
//! routing names out of loop files and avoids mixing tool execution with
//! backend selection.

/// Tool-capable request mode for harness evidence decisions.
///
/// This call attaches provider primitive tool schemas and should use an
/// OpenAI-compatible backend for LM Studio.
pub(crate) const HARNESS_TOOL_DECISION_REQUEST_MODE: &str = "harness_tool_decision";

/// Final-answer request mode after harness evidence has been collected.
///
/// This call receives verified evidence and asks for final natural text. No
/// primitive tools are exposed during synthesis.
pub(crate) const HARNESS_SYNTHESIS_REQUEST_MODE: &str = "harness_synthesis";
