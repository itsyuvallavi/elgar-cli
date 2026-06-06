//! Context budget and rough token estimation helpers.
//!
//! These helpers estimate local prompt size before a provider request is sent.
//! Provider-reported usage after a request is handled in `token_accounting.rs`.

pub const DEFAULT_CONTEXT_BUDGET_TOKENS: u64 = 768;
pub(super) const MIN_TRIMMED_CONTEXT_TOKENS: u64 = 16;

/// Estimate tokens from bytes using the rough local rule: 4 bytes per token.
pub(super) fn estimate_tokens_from_bytes(bytes: u64) -> u64 {
    bytes.div_ceil(4)
}

/// Choose how much of the context window local context may use.
pub fn context_budget_tokens(max_window_tokens: Option<u64>) -> u64 {
    match max_window_tokens {
        Some(max) => DEFAULT_CONTEXT_BUDGET_TOKENS.min(max.saturating_sub(256)),
        None => DEFAULT_CONTEXT_BUDGET_TOKENS,
    }
}

/// Trim text to an estimated token budget while preserving UTF-8 boundaries.
pub(super) fn truncate_to_estimated_tokens(content: &str, tokens: u64) -> String {
    let max_bytes = tokens.saturating_mul(4) as usize;
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_string()
}
