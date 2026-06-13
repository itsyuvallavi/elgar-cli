//! Guard provider final prose against unverified local-action claims.
//!
//! This module validates provider output, not user intent. The model remains
//! responsible for choosing tools; the harness only prevents final prose from
//! claiming local project facts or completed primitive actions when the current
//! turn has no verified evidence.

use crate::{harness::harness_loop::state::types::Evidence, session::Session};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ProseClaimGuardDecision {
    Allow,
    Block { reason: &'static str },
}

pub(super) fn validate_provider_final_text(
    session: &Session,
    content: &str,
    evidence: &[Evidence],
) -> ProseClaimGuardDecision {
    if !evidence.is_empty() || session.pending_approval().is_some() {
        return ProseClaimGuardDecision::Allow;
    }

    let normalized = normalize(content);
    if !has_local_project_signal(&normalized) {
        return ProseClaimGuardDecision::Allow;
    }

    if claims_completed_local_action(&normalized) || claims_local_project_fact(&normalized) {
        return ProseClaimGuardDecision::Block {
            reason: "unverified_provider_action_claim",
        };
    }

    ProseClaimGuardDecision::Allow
}

fn normalize(content: &str) -> String {
    content
        .chars()
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>()
}

fn has_local_project_signal(text: &str) -> bool {
    text.contains("file")
        || text.contains("directory")
        || text.contains("folder")
        || text.contains("project")
        || text.contains("package.json")
        || text.contains("app/")
        || text.contains("src/")
        || text.contains(".rs")
        || text.contains(".ts")
        || text.contains(".tsx")
        || text.contains(".js")
        || text.contains(".jsx")
        || text.contains(".json")
        || text.contains(".md")
}

fn claims_completed_local_action(text: &str) -> bool {
    [
        "i read",
        "i've read",
        "i have read",
        "read successfully",
        "successfully read",
        "i listed",
        "i've listed",
        "i have listed",
        "listed the",
        "i searched",
        "i've searched",
        "i have searched",
        "searched the",
        "i grepped",
        "i inspected",
        "i've inspected",
        "i have inspected",
        "inspected the",
        "i checked",
        "i've checked",
        "i have checked",
        "checked the",
        "i wrote",
        "i've written",
        "i have written",
        "wrote the",
        "i created",
        "i've created",
        "i have created",
        "created the",
        "i updated",
        "i've updated",
        "i have updated",
        "updated the",
        "i edited",
        "i've edited",
        "i have edited",
        "edited the",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

fn claims_local_project_fact(text: &str) -> bool {
    [
        " contains ",
        " includes ",
        " exports ",
        " imports ",
        " defines ",
        " uses ",
        " has ",
        " is configured",
        " configuration",
    ]
    .iter()
    .any(|phrase| text.contains(phrase))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn session() -> Session {
        Session::new(
            "prose-claim-guard-test",
            Path::new("/tmp/prose-claim-guard-test"),
            Path::new("/tmp/prose-claim-guard-test"),
        )
    }

    #[test]
    fn allows_plain_chat_without_local_claims() {
        assert_eq!(
            validate_provider_final_text(&session(), "This is a direct answer.", &[]),
            ProseClaimGuardDecision::Allow
        );
    }

    #[test]
    fn blocks_local_action_claim_without_evidence() {
        assert_eq!(
            validate_provider_final_text(&session(), "I read package.json successfully.", &[]),
            ProseClaimGuardDecision::Block {
                reason: "unverified_provider_action_claim"
            }
        );
    }

    #[test]
    fn blocks_local_project_fact_without_evidence() {
        assert_eq!(
            validate_provider_final_text(
                &session(),
                "The app/page.tsx file exports a default component.",
                &[],
            ),
            ProseClaimGuardDecision::Block {
                reason: "unverified_provider_action_claim"
            }
        );
    }
}
