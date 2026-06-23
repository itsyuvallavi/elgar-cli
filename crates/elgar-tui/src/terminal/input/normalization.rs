//! Cleans pasted terminal transcript text before submission.
//!
//! This prevents copied `user:`/`assistant:` transcript prefixes from becoming
//! part of the next prompt.

use std::borrow::Cow;

/// Strip prompt text copied from Elgar transcripts before sending input to the
/// provider. This is UI cleanup, not intent routing.
pub(super) fn normalize_pasted_transcript_input(input: &str) -> Cow<'_, str> {
    let stripped = strip_pasted_transcript_markers(input);
    if stripped.len() == input.len() {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(stripped.to_string())
    }
}

fn strip_pasted_transcript_markers(input: &str) -> &str {
    let mut stripped = input.trim_start();

    loop {
        let before = stripped;

        while let Some(rest) = stripped.strip_prefix('>') {
            stripped = rest.trim_start();
        }

        for prefix in ["user:", "you:", "human:", "me:"] {
            if let Some(rest) = strip_ascii_case_prefix(stripped, prefix) {
                stripped = rest.trim_start();
                break;
            }
        }

        if stripped == before {
            return stripped;
        }
    }
}

fn strip_ascii_case_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let head = input.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &input[prefix.len()..])
}
