//! Guard provider prose against incorrect approval requests.
//!
//! This module validates provider output only. It catches cases where the
//! provider asks for approval to use read-only primitives that policy allows
//! without approval, or where provider prose asks the user to approve a risky
//! action but the harness has no pending approval record.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ApprovalClaimGuardDecision {
    Allow,
    Block { reason: &'static str },
}

pub(super) fn validate_approval_claim(
    content: &str,
    has_pending_approval: bool,
) -> ApprovalClaimGuardDecision {
    let normalized = normalize(content);
    if asks_approval_for_read_only_tool(&normalized) {
        return ApprovalClaimGuardDecision::Block {
            reason: "read_only_approval_claim",
        };
    }
    if !has_pending_approval && asks_approval_for_side_effect(&normalized) {
        return ApprovalClaimGuardDecision::Block {
            reason: "approval_claim_without_pending_approval",
        };
    }

    ApprovalClaimGuardDecision::Allow
}

fn normalize(content: &str) -> String {
    content
        .chars()
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>()
}

fn asks_approval_for_read_only_tool(text: &str) -> bool {
    asks_for_approval(text) && mentions_read_only_tool(text)
}

fn asks_approval_for_side_effect(text: &str) -> bool {
    asks_for_approval(text)
        && (mentions_side_effect_primitive(text) || mentions_shell_command(text))
}

fn asks_for_approval(text: &str) -> bool {
    text.contains("approval")
        || text.contains("permission")
        || text.contains("approve")
        || text.contains("should i proceed")
}

fn mentions_read_only_tool(text: &str) -> bool {
    text.contains(" read ")
        || text.contains(" read the ")
        || text.contains(" list ")
        || text.contains(" list the ")
        || text.contains(" grep ")
        || text.contains(" search ")
        || text.contains(" find ")
        || text.contains(" inspect ")
}

fn mentions_side_effect_primitive(text: &str) -> bool {
    text.contains(" bash ")
        || text.contains(" write ")
        || text.contains(" edit ")
        || text.contains(" execute ")
        || text.contains(" executing ")
        || text.contains(" command")
}

fn mentions_shell_command(text: &str) -> bool {
    text.contains("`mkdir ")
        || text.contains("`mv ")
        || text.contains("`cp ")
        || text.contains("`rm ")
        || text.contains("`touch ")
        || text.contains("`cat >")
        || text.contains("`echo ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_approval_request_for_read() {
        assert_eq!(
            validate_approval_claim(
                "I need your approval to read the `postcss.config.mjs` file. Should I proceed?",
                false,
            ),
            ApprovalClaimGuardDecision::Block {
                reason: "read_only_approval_claim"
            }
        );
    }

    #[test]
    fn allows_write_approval_request() {
        assert_eq!(
            validate_approval_claim(
                "I need your approval to create the file. Should I proceed?",
                true,
            ),
            ApprovalClaimGuardDecision::Allow
        );
    }

    #[test]
    fn blocks_side_effect_approval_request_without_pending_approval() {
        assert_eq!(
            validate_approval_claim(
                "Approval is required before executing this command. Please approve `mkdir beta gamma delta`.",
                false,
            ),
            ApprovalClaimGuardDecision::Block {
                reason: "approval_claim_without_pending_approval"
            }
        );
    }

    #[test]
    fn allows_side_effect_approval_request_with_pending_approval() {
        assert_eq!(
            validate_approval_claim(
                "Approval is required before executing this command. Please approve `mkdir alpha`.",
                true,
            ),
            ApprovalClaimGuardDecision::Allow
        );
    }
}
