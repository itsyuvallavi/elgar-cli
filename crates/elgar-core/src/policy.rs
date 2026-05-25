use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// Runtime-owned permission policy mode.
///
/// Selecting a mode controls whether validated model tool calls can apply
/// immediately or must wait for explicit user review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionPolicyMode {
    #[default]
    ReviewAll,
    AutoCreateReviewModify,
    WorkspaceWriteWithReview,
    FullAccess,
}

impl PermissionPolicyMode {
    pub const ALL: [Self; 4] = [
        Self::ReviewAll,
        Self::AutoCreateReviewModify,
        Self::WorkspaceWriteWithReview,
        Self::FullAccess,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReviewAll => "review_all",
            Self::AutoCreateReviewModify => "auto_create_review_modify",
            Self::WorkspaceWriteWithReview => "workspace_write_with_review",
            Self::FullAccess => "full_access",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PermissionPolicyModeParseError> {
        value.parse()
    }

    pub fn next(self) -> Self {
        let index = Self::ALL
            .iter()
            .position(|mode| *mode == self)
            .unwrap_or_default();
        Self::ALL[(index + 1) % Self::ALL.len()]
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::ReviewAll => "review every file and shell action",
            Self::AutoCreateReviewModify => {
                "auto-create files and folders; review edits, deletes, moves, and shell"
            }
            Self::WorkspaceWriteWithReview => {
                "auto-apply workspace file writes; review deletes, moves, and shell"
            }
            Self::FullAccess => "auto-apply validated file and shell actions",
        }
    }
}

impl fmt::Display for PermissionPolicyMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for PermissionPolicyMode {
    type Err = PermissionPolicyModeParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().replace('-', "_").to_ascii_lowercase();
        match normalized.as_str() {
            "review_all" => Ok(Self::ReviewAll),
            "auto_create_review_modify" => Ok(Self::AutoCreateReviewModify),
            "workspace_write_with_review" => Ok(Self::WorkspaceWriteWithReview),
            "full_access" => Ok(Self::FullAccess),
            _ => Err(PermissionPolicyModeParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionPolicyModeParseError {
    pub value: String,
}

impl fmt::Display for PermissionPolicyModeParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid permission policy mode `{}`; expected review_all, auto_create_review_modify, workspace_write_with_review, or full_access",
            self.value
        )
    }
}

impl std::error::Error for PermissionPolicyModeParseError {}

/// Policy decision outcome for a validated action or tool request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecisionKind {
    AllowApply,
    #[default]
    RequireReview,
    Reject,
}

/// Audit source for an approval.
///
/// `User` means explicit user approval. `Policy` means the permission policy
/// approved apply without a manual approval step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum ApprovalSource {
    User,
    Policy {
        mode: PermissionPolicyMode,
        reason: String,
    },
}

impl ApprovalSource {
    pub fn user() -> Self {
        Self::User
    }

    pub fn policy(mode: PermissionPolicyMode, reason: impl Into<String>) -> Self {
        Self::Policy {
            mode,
            reason: reason.into(),
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Self::User)
    }

    pub fn is_policy(&self) -> bool {
        matches!(self, Self::Policy { .. })
    }
}

/// Data-only policy decision record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyDecision {
    pub mode: PermissionPolicyMode,
    pub kind: PolicyDecisionKind,
    pub reason: String,
    pub user_approval_required: bool,
    pub filesystem_verification_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_source: Option<ApprovalSource>,
}

impl PolicyDecision {
    pub fn allow_apply(mode: PermissionPolicyMode, reason: impl Into<String>) -> Self {
        let reason = reason.into();
        Self {
            mode,
            kind: PolicyDecisionKind::AllowApply,
            reason: reason.clone(),
            user_approval_required: false,
            filesystem_verification_required: true,
            approval_source: Some(ApprovalSource::policy(mode, reason)),
        }
    }

    pub fn require_review(mode: PermissionPolicyMode, reason: impl Into<String>) -> Self {
        Self {
            mode,
            kind: PolicyDecisionKind::RequireReview,
            reason: reason.into(),
            user_approval_required: true,
            filesystem_verification_required: true,
            approval_source: None,
        }
    }

    pub fn reject(mode: PermissionPolicyMode, reason: impl Into<String>) -> Self {
        Self {
            mode,
            kind: PolicyDecisionKind::Reject,
            reason: reason.into(),
            user_approval_required: false,
            filesystem_verification_required: false,
            approval_source: None,
        }
    }

    pub fn is_policy_approved(&self) -> bool {
        self.kind == PolicyDecisionKind::AllowApply
            && self
                .approval_source
                .as_ref()
                .is_some_and(ApprovalSource::is_policy)
    }
}

impl Default for PolicyDecision {
    fn default() -> Self {
        Self::require_review(
            PermissionPolicyMode::default(),
            "review required by default policy",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{ApprovalSource, PermissionPolicyMode, PolicyDecision, PolicyDecisionKind};
    use serde_json::json;

    #[test]
    fn policy_defaults_require_review_all() {
        let decision = PolicyDecision::default();

        assert_eq!(
            PermissionPolicyMode::default(),
            PermissionPolicyMode::ReviewAll
        );
        assert_eq!(decision.mode, PermissionPolicyMode::ReviewAll);
        assert_eq!(decision.kind, PolicyDecisionKind::RequireReview);
        assert!(decision.user_approval_required);
        assert!(decision.filesystem_verification_required);
        assert_eq!(decision.approval_source, None);
    }

    #[test]
    fn policy_mode_serde_names_roundtrip() {
        let value = serde_json::to_value([
            PermissionPolicyMode::ReviewAll,
            PermissionPolicyMode::AutoCreateReviewModify,
            PermissionPolicyMode::WorkspaceWriteWithReview,
            PermissionPolicyMode::FullAccess,
        ])
        .expect("serialize policy modes");

        assert_eq!(
            value,
            json!([
                "review_all",
                "auto_create_review_modify",
                "workspace_write_with_review",
                "full_access"
            ])
        );

        let modes: Vec<PermissionPolicyMode> =
            serde_json::from_value(value).expect("deserialize policy modes");
        assert_eq!(
            modes,
            vec![
                PermissionPolicyMode::ReviewAll,
                PermissionPolicyMode::AutoCreateReviewModify,
                PermissionPolicyMode::WorkspaceWriteWithReview,
                PermissionPolicyMode::FullAccess
            ]
        );
    }

    #[test]
    fn policy_mode_parses_display_names_and_cli_friendly_aliases() {
        assert_eq!(
            PermissionPolicyMode::parse("review_all").unwrap(),
            PermissionPolicyMode::ReviewAll
        );
        assert_eq!(
            PermissionPolicyMode::parse("auto-create-review-modify").unwrap(),
            PermissionPolicyMode::AutoCreateReviewModify
        );
        assert_eq!(
            PermissionPolicyMode::parse(" WORKSPACE_WRITE_WITH_REVIEW ").unwrap(),
            PermissionPolicyMode::WorkspaceWriteWithReview
        );
        assert_eq!(PermissionPolicyMode::FullAccess.to_string(), "full_access");

        let error = PermissionPolicyMode::parse("danger").unwrap_err();
        assert!(error.to_string().contains("invalid permission policy mode"));
    }

    #[test]
    fn policy_modes_cycle_in_display_order() {
        assert_eq!(
            PermissionPolicyMode::ReviewAll.next(),
            PermissionPolicyMode::AutoCreateReviewModify
        );
        assert_eq!(
            PermissionPolicyMode::AutoCreateReviewModify.next(),
            PermissionPolicyMode::WorkspaceWriteWithReview
        );
        assert_eq!(
            PermissionPolicyMode::WorkspaceWriteWithReview.next(),
            PermissionPolicyMode::FullAccess
        );
        assert_eq!(
            PermissionPolicyMode::FullAccess.next(),
            PermissionPolicyMode::ReviewAll
        );
    }

    #[test]
    fn policy_decision_kind_serde_names_roundtrip() {
        let value = serde_json::to_value([
            PolicyDecisionKind::AllowApply,
            PolicyDecisionKind::RequireReview,
            PolicyDecisionKind::Reject,
        ])
        .expect("serialize decision kinds");

        assert_eq!(value, json!(["allow_apply", "require_review", "reject"]));

        let kinds: Vec<PolicyDecisionKind> =
            serde_json::from_value(value).expect("deserialize decision kinds");
        assert_eq!(
            kinds,
            vec![
                PolicyDecisionKind::AllowApply,
                PolicyDecisionKind::RequireReview,
                PolicyDecisionKind::Reject
            ]
        );
    }

    #[test]
    fn approval_source_distinguishes_user_from_policy_approval() {
        let user = ApprovalSource::user();
        let policy = ApprovalSource::policy(
            PermissionPolicyMode::AutoCreateReviewModify,
            "validated new file create",
        );

        assert!(user.is_user());
        assert!(!user.is_policy());
        assert!(policy.is_policy());
        assert!(!policy.is_user());
        assert_ne!(user, policy);
        assert_eq!(
            serde_json::to_value(user).expect("serialize user"),
            json!({
                "source": "user"
            })
        );
        assert_eq!(
            serde_json::to_value(policy).expect("serialize policy"),
            json!({
                "source": "policy",
                "mode": "auto_create_review_modify",
                "reason": "validated new file create"
            })
        );
    }

    #[test]
    fn policy_decision_roundtrips_with_policy_approval_source() {
        let decision = PolicyDecision::allow_apply(
            PermissionPolicyMode::AutoCreateReviewModify,
            "new project-relative file validated under allowed root",
        );

        assert!(decision.is_policy_approved());
        assert!(!decision.user_approval_required);
        assert!(decision.filesystem_verification_required);

        let value = serde_json::to_value(&decision).expect("serialize decision");
        assert_eq!(
            value,
            json!({
                "mode": "auto_create_review_modify",
                "kind": "allow_apply",
                "reason": "new project-relative file validated under allowed root",
                "user_approval_required": false,
                "filesystem_verification_required": true,
                "approval_source": {
                    "source": "policy",
                    "mode": "auto_create_review_modify",
                    "reason": "new project-relative file validated under allowed root"
                }
            })
        );

        let roundtrip: PolicyDecision =
            serde_json::from_value(value).expect("deserialize decision");
        assert_eq!(roundtrip, decision);
    }

    #[test]
    fn require_review_and_reject_do_not_claim_approval_source() {
        let review = PolicyDecision::require_review(
            PermissionPolicyMode::ReviewAll,
            "all actions require review",
        );
        let reject = PolicyDecision::reject(
            PermissionPolicyMode::FullAccess,
            "target outside allowed roots",
        );

        assert_eq!(review.kind, PolicyDecisionKind::RequireReview);
        assert!(review.user_approval_required);
        assert!(review.filesystem_verification_required);
        assert_eq!(review.approval_source, None);
        assert!(!review.is_policy_approved());

        assert_eq!(reject.kind, PolicyDecisionKind::Reject);
        assert!(!reject.user_approval_required);
        assert!(!reject.filesystem_verification_required);
        assert_eq!(reject.approval_source, None);
        assert!(!reject.is_policy_approved());
    }
}
