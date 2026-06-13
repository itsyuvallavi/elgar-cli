//! Explicit primitive target fidelity checks.
//!
//! This module does not route user intent or create tool requests. It only
//! rejects provider-selected primitive arguments that obviously conflict with a
//! narrow direct primitive request already made by the user.

use crate::harness::{
    harness_loop::evidence::{
        keys::normalize_evidence_path,
        request_args::{request_path, request_query},
    },
    StructuredRequestKind, ValidatedStructuredRequest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolTargetMismatch {
    pub reason: &'static str,
    pub notice: String,
}

pub(super) fn validate_tool_target(
    input: &str,
    request: &ValidatedStructuredRequest,
) -> Option<ToolTargetMismatch> {
    let expected = explicit_primitive_request(input)?;
    match expected {
        ExplicitPrimitiveRequest::Read { path } => validate_read_target(&path, request),
        ExplicitPrimitiveRequest::Grep { query, path } => {
            validate_grep_target(&query, &path, request)
        }
    }
}

fn validate_read_target(
    expected_path: &str,
    request: &ValidatedStructuredRequest,
) -> Option<ToolTargetMismatch> {
    let actual_path = request_path(request).map(normalize_evidence_path);
    if request.kind == StructuredRequestKind::Read && actual_path.as_deref() == Some(expected_path)
    {
        return None;
    }

    Some(ToolTargetMismatch {
        reason: "tool_target_mismatch",
        notice: format!(
            "VERIFIED_LOOP_NOTICE\nTool target mismatch rejected. User explicitly requested file evidence for `{expected_path}`, but provider selected `{}` with path `{}`. Request the matching primitive tool or answer without claiming evidence.",
            request.kind.as_str(),
            actual_path.as_deref().unwrap_or("(missing)")
        ),
    })
}

fn validate_grep_target(
    expected_query: &str,
    expected_path: &str,
    request: &ValidatedStructuredRequest,
) -> Option<ToolTargetMismatch> {
    let actual_path = request_path(request).map(normalize_evidence_path);
    let actual_query = request_query(request).map(str::trim);
    if request.kind == StructuredRequestKind::Grep
        && actual_path.as_deref() == Some(expected_path)
        && actual_query == Some(expected_query)
    {
        return None;
    }

    Some(ToolTargetMismatch {
        reason: "tool_target_mismatch",
        notice: format!(
            "VERIFIED_LOOP_NOTICE\nTool target mismatch rejected. User explicitly requested text search `{expected_query}` in `{expected_path}`, but provider selected `{}` with path `{}` and query `{}`. Request the matching primitive tool or answer without claiming evidence.",
            request.kind.as_str(),
            actual_path.as_deref().unwrap_or("(missing)"),
            actual_query.unwrap_or("(missing)")
        ),
    })
}

pub(super) enum ExplicitPrimitiveRequest {
    Read { path: String },
    Grep { query: String, path: String },
}

impl ExplicitPrimitiveRequest {
    pub(super) fn missing_evidence_feedback(&self) -> String {
        format!(
            "The user directly requested `{}`. This turn has no verified tool evidence. Request the matching primitive tool now. Do not answer from memory, prior chat, or claim the path exists or does not exist without verified tool evidence.",
            self.request_label()
        )
    }

    fn request_label(&self) -> String {
        match self {
            Self::Read { path } => format!("file evidence for {path}"),
            Self::Grep { query, path } => format!("search for {query} in {path}"),
        }
    }
}

pub(super) fn explicit_primitive_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    if let Some(request) = explicit_read_request(input) {
        return Some(request);
    }
    explicit_grep_request(input)
}

fn explicit_read_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let trimmed = input.trim();
    let read_path = strip_any_prefix(trimmed, &["read ", "open ", "show me "])?;
    let path = strip_wrapping_punctuation(read_path);
    if !path.contains(' ') && is_file_like_path(path) {
        return Some(ExplicitPrimitiveRequest::Read {
            path: normalize_evidence_path(path),
        });
    }

    None
}

fn explicit_grep_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let trimmed = input.trim();
    if let Some(request) = explicit_search_inside_request(trimmed) {
        return Some(request);
    }
    let rest = strip_any_prefix(trimmed, &["grep ", "search for ", "find ", "look for "])?;
    let (query, path) = rest.split_once(" in ")?;
    if query.trim().contains(' ') || path.trim().contains(' ') {
        return None;
    }
    Some(ExplicitPrimitiveRequest::Grep {
        query: strip_wrapping_punctuation(query).trim().to_string(),
        path: normalize_evidence_path(strip_wrapping_punctuation(path)),
    })
}

fn explicit_search_inside_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let rest = input.strip_prefix("search inside ")?;
    let (path, query) = rest.split_once(" for ")?;
    if query.trim().contains(' ') || path.trim().contains(' ') {
        return None;
    }
    Some(ExplicitPrimitiveRequest::Grep {
        query: strip_wrapping_punctuation(query).trim().to_string(),
        path: normalize_evidence_path(strip_wrapping_punctuation(path)),
    })
}

fn strip_any_prefix<'a>(value: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
}

fn strip_wrapping_punctuation(value: &str) -> &str {
    value
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
}

fn is_file_like_path(path: &str) -> bool {
    path.contains('/')
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn accepts_matching_read() {
        let request = request(
            StructuredRequestKind::Read,
            json!({"path":"./package.json"}),
        );
        assert!(validate_tool_target("read package.json", &request).is_none());
    }

    #[test]
    fn rejects_wrong_read_target() {
        let request = request(StructuredRequestKind::Read, json!({"path":"app/page.tsx"}));
        assert!(validate_tool_target("read postcss.config.mjs", &request).is_some());
    }

    #[test]
    fn rejects_wrong_grep_target() {
        let request = request(
            StructuredRequestKind::Grep,
            json!({"path":".","query":"tailwind"}),
        );
        assert!(validate_tool_target("grep tailwind in tailwind.config.ts", &request).is_some());
    }

    #[test]
    fn rejects_wrong_search_target() {
        let request = request(
            StructuredRequestKind::Find,
            json!({"path":".","pattern":"*config*"}),
        );
        assert!(
            validate_tool_target("search for tailwind in tailwind.config.ts", &request).is_some()
        );
    }

    #[test]
    fn accepts_matching_search_target() {
        let request = request(
            StructuredRequestKind::Grep,
            json!({"path":"tailwind.config.ts","query":"tailwind"}),
        );
        assert!(
            validate_tool_target("search for tailwind in tailwind.config.ts", &request).is_none()
        );
    }

    #[test]
    fn accepts_search_inside_target() {
        let request = request(
            StructuredRequestKind::Grep,
            json!({"path":"tailwind.config.ts","query":"tailwind"}),
        );
        assert!(
            validate_tool_target("search inside tailwind.config.ts for tailwind", &request)
                .is_none()
        );
    }

    #[test]
    fn accepts_user_language_file_read() {
        let request = request(
            StructuredRequestKind::Read,
            json!({"path":"postcss.config.mjs"}),
        );
        assert!(validate_tool_target("show me postcss.config.mjs", &request).is_none());
    }

    fn request(
        kind: StructuredRequestKind,
        arguments: serde_json::Value,
    ) -> ValidatedStructuredRequest {
        ValidatedStructuredRequest {
            kind,
            reason: "test".to_string(),
            arguments: Some(arguments),
        }
    }
}
