use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// Explicit local controller routes.
///
/// Normal user text is not classified here. It belongs to the provider/model
/// path unless a slash command or typed runtime state handles it elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    AskModel,
    ProposeMarkdownPlanFile,
    ProposeWriteFile,
    ProposePatchFile,
    ProposeOverwriteFile,
    ProposeDeleteFile,
    ProposeMoveFile,
    ProposeCreateDirectory,
    ProposeShellCommand,
    ExecutePlan,
    ApproveAction,
    RejectAction,
    Help,
    Unknown,
}

/// Classify only explicit slash/local commands.
pub fn route_input(input: &str) -> Route {
    let input = normalize_pasted_transcript_input(input);
    let normalized = input.trim();

    if normalized.is_empty() {
        return Route::Unknown;
    }

    match normalized {
        "/help" | "/commands" => Route::Help,
        "/approve" => Route::ApproveAction,
        "/reject" => Route::RejectAction,
        _ => Route::AskModel,
    }
}

/// Strip prompt text copied from Elgar transcripts before explicit command
/// parsing. This does not route ordinary words to actions.
pub fn normalize_pasted_transcript_input(input: &str) -> Cow<'_, str> {
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

#[cfg(test)]
mod tests {
    use super::{normalize_pasted_transcript_input, route_input, Route};

    #[test]
    fn routes_only_explicit_slash_commands_locally() {
        assert_eq!(route_input("/help"), Route::Help);
        assert_eq!(route_input("/commands"), Route::Help);
        assert_eq!(route_input("/approve"), Route::ApproveAction);
        assert_eq!(route_input("/reject"), Route::RejectAction);
    }

    #[test]
    fn ordinary_words_go_to_model_path() {
        assert_eq!(route_input("help"), Route::AskModel);
        assert_eq!(route_input("approve"), Route::AskModel);
        assert_eq!(route_input("yes"), Route::AskModel);
        assert_eq!(route_input("create file hello.py"), Route::AskModel);
        assert_eq!(route_input("create a folder called demo"), Route::AskModel);
    }

    #[test]
    fn empty_input_remains_unknown() {
        assert_eq!(route_input(""), Route::Unknown);
        assert_eq!(route_input("   "), Route::Unknown);
    }

    #[test]
    fn transcript_prefixes_are_removed_before_slash_command_parsing() {
        assert_eq!(
            normalize_pasted_transcript_input("> User: /approve").as_ref(),
            "/approve"
        );
        assert_eq!(route_input("> User: /reject"), Route::RejectAction);
    }
}
