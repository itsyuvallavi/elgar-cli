//! Verified evidence helpers for the primitive harness loop.
//!
//! Evidence here only comes from Rust collectors. Provider prose is never
//! promoted into verified evidence.

use std::path::{Component, Path};

use serde_json::Value;

use crate::{
    harness::{
        collect_directory_summary, collect_find_matches, collect_grep_matches,
        collect_project_file, DirectoryOptions, FindOptions, GrepOptions, ModelChoiceTurnError,
        PermissionDecision, ProjectFileOptions, StructuredRequestKind, ValidatedStructuredRequest,
    },
    session::Session,
};

use crate::harness::harness_loop::state::{
    listing_memory::DirectoryListingMemory,
    types::{Evidence, EvidenceKey},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct ExecutedEvidence {
    pub evidence: Evidence,
    pub directory_listing: Option<DirectoryListingMemory>,
}

/// Execute one validated primitive request and return verified evidence.
pub(in crate::harness::harness_loop) fn execute_read_only_request(
    session: &Session,
    request: &ValidatedStructuredRequest,
) -> Result<ExecutedEvidence, ModelChoiceTurnError> {
    match request.kind {
        StructuredRequestKind::Read => {
            let path = request_path(request).unwrap_or_default();
            let label_path = normalize_evidence_path(path);
            let snapshot = collect_project_file(&session.cwd, path, ProjectFileOptions::default())
                .map_err(|error| ModelChoiceTurnError::ProjectFile(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("read:{label_path}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Ls => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let snapshot =
                collect_directory_summary(&session.cwd, path, DirectoryOptions::default())
                    .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let directory_listing =
                DirectoryListingMemory::from_snapshot(label_path.clone(), &snapshot);
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("ls:{label_path}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated || snapshot.count_truncated,
                    body,
                },
                directory_listing: Some(directory_listing),
            })
        }
        StructuredRequestKind::Find => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let pattern = request_pattern(request).unwrap_or_default();
            let snapshot =
                collect_find_matches(&session.cwd, path, pattern, FindOptions::default())
                    .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("find:{label_path}:{pattern}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Grep => {
            let path = request_path(request).unwrap_or(".");
            let label_path = normalize_evidence_path(path);
            let query = request_query(request).unwrap_or_default();
            let snapshot = collect_grep_matches(&session.cwd, path, query, GrepOptions::default())
                .map_err(|error| ModelChoiceTurnError::ProjectContext(error.to_string()))?;
            let body = snapshot.render_for_model();
            Ok(ExecutedEvidence {
                evidence: Evidence {
                    label: format!("grep:{label_path}:{query}"),
                    bytes: body.len(),
                    truncated: snapshot.truncated,
                    body,
                },
                directory_listing: None,
            })
        }
        StructuredRequestKind::Bash
        | StructuredRequestKind::Write
        | StructuredRequestKind::Edit => Ok(ExecutedEvidence {
            evidence: Evidence {
                label: request.kind.as_str().to_string(),
                body: format!(
                    "Model requested primitive {}, which is declared but not executable in this stage.",
                    request.kind.as_str()
                ),
                bytes: 0,
                truncated: false,
            },
            directory_listing: None,
        }),
    }
}

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

/// Convert a failed execution into verified error evidence for synthesis.
pub(in crate::harness::harness_loop) fn error_evidence(label: String, error: &str) -> Evidence {
    let body = format!(
        "VERIFIED_EXECUTION_ERROR\nlabel: {label}\nerror: {error}\nfile_contents_read: false\n"
    );
    Evidence {
        label,
        bytes: body.len(),
        truncated: false,
        body,
    }
}

/// Convert a blocked permission decision into verified evidence.
pub(in crate::harness::harness_loop) fn permission_evidence(
    label: String,
    request: &ValidatedStructuredRequest,
    decision: &PermissionDecision,
) -> Evidence {
    let body = format!(
        "VERIFIED_PERMISSION_DECISION\ntool: {}\ndecision: {}\nreason: {}\nexecution_performed: false\n",
        request.kind.as_str(),
        decision.kind.as_str(),
        decision.reason.as_str()
    );
    Evidence {
        label,
        bytes: body.len(),
        truncated: false,
        body,
    }
}

/// Render verified evidence blocks for final synthesis.
pub(in crate::harness::harness_loop) fn render_evidence_for_synthesis(
    evidence: &[Evidence],
) -> String {
    if evidence.is_empty() {
        return "(none)".to_string();
    }

    let mut rendered = String::new();
    for item in evidence {
        rendered.push_str("\n--- Verified Evidence: ");
        rendered.push_str(&item.label);
        rendered.push_str(" ---\n");
        rendered.push_str("truncated: ");
        rendered.push_str(if item.truncated { "true" } else { "false" });
        rendered.push('\n');
        rendered.push_str(&item.body);
        rendered.push('\n');
    }
    rendered
}

fn request_path(request: &ValidatedStructuredRequest) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("path"))
        .and_then(Value::as_str)
}

fn request_pattern(request: &ValidatedStructuredRequest) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("pattern"))
        .and_then(Value::as_str)
}

fn request_query(request: &ValidatedStructuredRequest) -> Option<&str> {
    request
        .arguments
        .as_ref()
        .and_then(|value: &Value| value.get("query"))
        .and_then(Value::as_str)
}

fn normalize_evidence_path(path: &str) -> String {
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
