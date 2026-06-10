//! Permission decisions for risky harness primitives.
//!
//! This module owns whether a primitive request may execute, needs approval, or
//! must be denied. It does not execute tools and does not ask the user.

mod approval;
mod policy;
mod types;

pub use approval::{PendingApproval, PendingApprovalStatus};
pub use policy::decide_primitive_permission;
pub use types::{PermissionDecision, PermissionDecisionKind};
