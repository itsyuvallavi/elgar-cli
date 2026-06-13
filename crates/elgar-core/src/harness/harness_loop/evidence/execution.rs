//! Verified evidence helpers for the primitive harness loop.
//!
//! Evidence here only comes from Rust collectors. Provider prose is never
//! promoted into verified evidence.

use crate::{
    harness::{
        collect_directory_summary, collect_find_matches, collect_grep_matches,
        collect_project_file, DirectoryOptions, FindOptions, GrepOptions, ModelChoiceTurnError,
        ProjectFileOptions, StructuredRequestKind, ValidatedStructuredRequest,
    },
    session::Session,
};

use crate::harness::harness_loop::{
    evidence::{
        keys::normalize_evidence_path,
        request_args::{request_path, request_pattern, request_query},
    },
    state::{listing_memory::DirectoryListingMemory, types::Evidence},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct ExecutedEvidence {
    pub evidence: Evidence,
    pub directory_listing: Option<DirectoryListingMemory>,
}

/// Execute one validated primitive request and return verified evidence.
pub(in crate::harness::harness_loop) fn execute_primitive_request(
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
        | StructuredRequestKind::Edit => Err(ModelChoiceTurnError::ProjectContext(format!(
            "permission policy must handle primitive `{}` before execution",
            request.kind.as_str()
        ))),
        StructuredRequestKind::McpCall => {
            let evidence = super::mcp::execute_mcp_call_request(session, request)?;
            Ok(ExecutedEvidence {
                evidence,
                directory_listing: None,
            })
        }
    }
}
