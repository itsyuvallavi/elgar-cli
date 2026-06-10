//! Pending approval state for risky harness primitives.
//!
//! Approval state is runtime truth owned by core. UI surfaces may render or
//! update it, but they should not invent approval records.

use serde::{Deserialize, Serialize};

use crate::harness::ValidatedStructuredRequest;

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
    pub status: PendingApprovalStatus,
}

impl PendingApproval {
    pub fn from_request(
        id: impl Into<String>,
        request: &ValidatedStructuredRequest,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            tool: request.kind.as_str().to_string(),
            reason: reason.into(),
            arguments_preview: bounded_arguments_preview(request),
            status: PendingApprovalStatus::Pending,
        }
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
}
