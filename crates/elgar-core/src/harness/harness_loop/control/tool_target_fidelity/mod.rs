//! Explicit primitive target fidelity checks.
//!
//! This module does not route user intent or create tool requests. It only
//! rejects provider-selected primitive arguments that obviously conflict with a
//! narrow direct primitive request already made by the user.

mod intent;
mod path_match;

#[cfg(test)]
mod tests;

use crate::harness::{
    harness_loop::{
        evidence::{
            keys::normalize_evidence_path,
            request_args::{request_path, request_query},
        },
        state::types::Evidence,
    },
    StructuredRequestKind, ValidatedStructuredRequest,
};

pub(super) use intent::explicit_primitive_request;
use intent::ExplicitPrimitiveRequest;
use path_match::direct_path_matches;

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
        ExplicitPrimitiveRequest::List { path } => validate_list_target(&path, request),
        ExplicitPrimitiveRequest::Grep { query, path } => {
            validate_grep_target(&query, &path, request)
        }
    }
}

pub(super) fn direct_request_satisfied(input: &str, evidence: &[Evidence]) -> bool {
    let Some(request) = explicit_primitive_request(input) else {
        return false;
    };
    evidence
        .last()
        .is_some_and(|item| request.matches_evidence_label(&item.label))
}

fn validate_read_target(
    expected_path: &str,
    request: &ValidatedStructuredRequest,
) -> Option<ToolTargetMismatch> {
    let actual_path = request_path(request).map(normalize_evidence_path);
    if request.kind == StructuredRequestKind::Read
        && actual_path
            .as_deref()
            .is_some_and(|actual_path| direct_path_matches(expected_path, actual_path))
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

fn validate_list_target(
    expected_path: &str,
    request: &ValidatedStructuredRequest,
) -> Option<ToolTargetMismatch> {
    let actual_path = request_path(request).map(normalize_evidence_path);
    if request.kind == StructuredRequestKind::Ls
        && actual_path
            .as_deref()
            .is_some_and(|actual_path| direct_path_matches(expected_path, actual_path))
    {
        return None;
    }

    Some(ToolTargetMismatch {
        reason: "tool_target_mismatch",
        notice: format!(
            "VERIFIED_LOOP_NOTICE\nTool target mismatch rejected. User explicitly requested directory evidence for `{expected_path}`, but provider selected `{}` with path `{}`. Request the matching primitive tool or answer without claiming evidence.",
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

impl ExplicitPrimitiveRequest {
    fn matches_evidence_label(&self, label: &str) -> bool {
        match self {
            Self::Read { path } => label
                .strip_prefix("read:")
                .is_some_and(|actual| direct_path_matches(path, actual)),
            Self::List { path } => label
                .strip_prefix("ls:")
                .is_some_and(|actual| direct_path_matches(path, actual)),
            Self::Grep { query, path } => label
                .strip_prefix("grep:")
                .and_then(|rest| rest.rsplit_once(':'))
                .is_some_and(|(actual_path, actual_query)| {
                    actual_query == query && direct_path_matches(path, actual_path)
                }),
        }
    }
}
