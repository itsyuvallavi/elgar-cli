//! Permission decision data for harness primitive requests.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    #[default]
    ReviewAll,
    WorkspaceWrite,
    FullAccess,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReviewAll => "review_all",
            Self::WorkspaceWrite => "workspace_write",
            Self::FullAccess => "full_access",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecisionKind {
    Allow,
    NeedsApproval,
    Deny,
}

impl PermissionDecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::NeedsApproval => "needs_approval",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub kind: PermissionDecisionKind,
    pub reason: String,
}

impl PermissionDecision {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            kind: PermissionDecisionKind::Allow,
            reason: reason.into(),
        }
    }

    pub fn needs_approval(reason: impl Into<String>) -> Self {
        Self {
            kind: PermissionDecisionKind::NeedsApproval,
            reason: reason.into(),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            kind: PermissionDecisionKind::Deny,
            reason: reason.into(),
        }
    }

    pub fn allows_execution(&self) -> bool {
        matches!(self.kind, PermissionDecisionKind::Allow)
    }
}
