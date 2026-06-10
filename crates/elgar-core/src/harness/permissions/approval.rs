//! Pending approval state for risky harness primitives.
//!
//! Approval state is runtime truth owned by core. UI surfaces may render or
//! update it, but they should not invent approval records.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::harness::ValidatedStructuredRequest;

use super::approval_preview::{preview_request_target, ApprovalTargetPreview};

const ARGUMENT_PREVIEW_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingApprovalStatus {
    Pending,
    Approved,
    Denied,
}

impl PendingApprovalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub tool: String,
    pub reason: String,
    pub arguments_preview: String,
    pub target_preview: Option<ApprovalTargetPreview>,
    pub request: ValidatedStructuredRequest,
    pub status: PendingApprovalStatus,
}

impl PendingApproval {
    pub fn from_request(
        id: impl Into<String>,
        request: &ValidatedStructuredRequest,
        reason: impl Into<String>,
    ) -> Self {
        Self::build(id, request, reason, None)
    }

    pub fn from_request_with_launch_cwd(
        id: impl Into<String>,
        request: &ValidatedStructuredRequest,
        reason: impl Into<String>,
        launch_cwd: &Path,
    ) -> Self {
        Self::build(id, request, reason, Some(launch_cwd))
    }

    fn build(
        id: impl Into<String>,
        request: &ValidatedStructuredRequest,
        reason: impl Into<String>,
        launch_cwd: Option<&Path>,
    ) -> Self {
        Self {
            id: id.into(),
            tool: request.kind.as_str().to_string(),
            reason: reason.into(),
            arguments_preview: bounded_arguments_preview(request),
            target_preview: launch_cwd.and_then(|cwd| preview_request_target(cwd, request)),
            request: request.clone(),
            status: PendingApprovalStatus::Pending,
        }
    }

    pub fn approve(mut self) -> Self {
        self.status = PendingApprovalStatus::Approved;
        self
    }

    pub fn deny(mut self) -> Self {
        self.status = PendingApprovalStatus::Denied;
        self
    }
}

fn bounded_arguments_preview(request: &ValidatedStructuredRequest) -> String {
    let Some(arguments) = request.arguments.as_ref() else {
        return "{}".to_string();
    };
    let serialized = serde_json::to_string(arguments).unwrap_or_else(|_| "{}".to_string());
    if serialized.chars().count() <= ARGUMENT_PREVIEW_CHARS {
        return serialized;
    }

    let mut preview = serialized
        .chars()
        .take(ARGUMENT_PREVIEW_CHARS)
        .collect::<String>();
    preview.push_str("...");
    preview
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::harness::{StructuredRequestKind, ValidatedStructuredRequest};

    use super::{PendingApproval, PendingApprovalStatus};

    #[test]
    fn pending_approval_bounds_argument_preview() {
        let request = ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "test".to_string(),
            arguments: Some(json!({
                "path": "demo.txt",
                "content": "x".repeat(700)
            })),
        };

        let approval = PendingApproval::from_request("approval-1", &request, "needs approval");

        assert_eq!(approval.id, "approval-1");
        assert_eq!(approval.tool, "write");
        assert_eq!(approval.status, PendingApprovalStatus::Pending);
        assert!(approval.arguments_preview.chars().count() <= 503);
        assert!(approval.arguments_preview.ends_with("..."));
    }

    #[test]
    fn pending_approval_carries_target_preview_when_launch_cwd_is_known() {
        let request = ValidatedStructuredRequest {
            kind: StructuredRequestKind::Write,
            reason: "test".to_string(),
            arguments: Some(json!({
                "path": "demo.txt",
                "content": "hello"
            })),
        };

        let approval = PendingApproval::from_request_with_launch_cwd(
            "approval-1",
            &request,
            "needs approval",
            std::path::Path::new("/project"),
        );

        let preview = approval.target_preview.unwrap();
        assert_eq!(preview.requested_path, "demo.txt");
        assert_eq!(preview.resolved_preview_path, "/project/demo.txt");
    }
}
