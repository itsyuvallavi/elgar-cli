//! Stable evidence keys for primitive harness requests.

use std::path::{Component, Path};

use crate::{
    harness::harness_loop::{
        evidence::request_args::{request_path, request_pattern, request_query},
        state::types::EvidenceKey,
    },
    harness::{StructuredRequestKind, ValidatedStructuredRequest},
};

/// Build the stable budget key for one validated request.
pub(in crate::harness::harness_loop) fn evidence_key_for_request(
    request: &ValidatedStructuredRequest,
) -> EvidenceKey {
    match request.kind {
        StructuredRequestKind::Read => EvidenceKey::Read(normalize_evidence_path(
            request_path(request).unwrap_or_default(),
        )),
        StructuredRequestKind::Ls => EvidenceKey::Ls(normalize_evidence_path(
            request_path(request).unwrap_or("."),
        )),
        StructuredRequestKind::Find => EvidenceKey::Find(
            normalize_evidence_path(request_path(request).unwrap_or(".")),
            request_pattern(request).unwrap_or_default().to_string(),
        ),
        StructuredRequestKind::Grep => EvidenceKey::Grep(
            normalize_evidence_path(request_path(request).unwrap_or(".")),
            request_query(request).unwrap_or_default().to_string(),
        ),
        StructuredRequestKind::Bash
        | StructuredRequestKind::Write
        | StructuredRequestKind::Edit => EvidenceKey::Primitive(request.kind.as_str().to_string()),
    }
}

pub(in crate::harness::harness_loop) fn normalize_evidence_path(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return ".".to_string();
    }

    let source = Path::new(trimmed);
    let mut parts = Vec::new();
    let mut absolute = false;
    for component in source.components() {
        match component {
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().into_owned());
            }
            Component::RootDir => {
                absolute = true;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last().map(String::as_str), Some("..")) || parts.is_empty() {
                    if !absolute {
                        parts.push("..".to_string());
                    }
                } else {
                    parts.pop();
                }
            }
            Component::Normal(value) => {
                parts.push(value.to_string_lossy().into_owned());
            }
        }
    }

    let normalized = parts.join("/");
    if absolute {
        if normalized.is_empty() {
            "/".to_string()
        } else {
            format!("/{normalized}")
        }
    } else if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized
    }
}
