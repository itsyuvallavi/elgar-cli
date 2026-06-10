//! Permission decisions for risky harness primitives.
//!
//! This module owns whether a primitive request may execute, needs approval, or
//! must be denied. Approval command handling executes only explicitly approved
//! primitives.

mod approval;
mod approval_flow;
mod policy;
mod types;

pub use approval::{PendingApproval, PendingApprovalStatus};
pub use approval_flow::{
    approve_pending_approval, deny_pending_approval, ApprovalCommandError, ApprovalCommandResult,
};
pub use policy::decide_primitive_permission;
pub use types::{PermissionDecision, PermissionDecisionKind};
