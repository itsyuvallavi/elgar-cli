//! Approval button selection for the inline terminal.
//!
//! This is display/input state only. Core still owns approval truth and
//! execution.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovalAction {
    Approve,
    Deny,
}

impl ApprovalAction {
    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Approve => Self::Deny,
            Self::Deny => Self::Approve,
        }
    }
}
