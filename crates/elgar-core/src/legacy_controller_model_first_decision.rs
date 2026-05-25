use crate::{
    action::{Action, ActionRequest},
    legacy_controller_model_first_apply::model_first_request_is_safe_create,
    model_runtime::ValidatedModelToolAction,
    policy::{PermissionPolicyMode, PolicyDecision},
};

pub(crate) fn model_first_proposal_message(mode: PermissionPolicyMode) -> String {
    format!(
        "Model-first tool call validated under {mode:?}. Proposed action only. Approve or reject before anything changes."
    )
}

pub(crate) fn policy_decision_for_model_first_action(
    mode: PermissionPolicyMode,
    action: &Action,
) -> PolicyDecision {
    match (mode, &action.request) {
        (
            PermissionPolicyMode::AutoCreateReviewModify,
            ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "safe new create action validated by model-first tool call",
        ),
        (
            PermissionPolicyMode::WorkspaceWriteWithReview,
            ActionRequest::CreateFile(_)
            | ActionRequest::CreateDirectory(_)
            | ActionRequest::OverwriteFile(_)
            | ActionRequest::PatchFile(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "safe workspace write action validated by model-first tool call",
        ),
        (PermissionPolicyMode::AutoCreateReviewModify, _) => PolicyDecision::require_review(
            mode,
            "modify, delete, move, and shell actions require review",
        ),
        (PermissionPolicyMode::WorkspaceWriteWithReview, _) => {
            PolicyDecision::require_review(mode, "delete, move, and shell actions require review")
        }
        _ => PolicyDecision::require_review(mode, "policy mode requires user review"),
    }
}

pub(crate) fn should_ask_guidance_for_prose_only_model_first(
    input: &str,
    provider_text: &str,
) -> bool {
    is_model_first_execution_like_request(input)
        || model_first_provider_text_claims_execution(provider_text)
}

fn is_model_first_execution_like_request(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    contains_any_word(
        normalized,
        &[
            "create",
            "implement",
            "make",
            "build",
            "scaffold",
            "write",
            "add",
            "edit",
            "delete",
            "move",
            "rename",
            "run",
        ],
    )
}

fn model_first_provider_text_claims_execution(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    contains_any(
        &normalized,
        &[
            "i created",
            "created ",
            "i wrote",
            "wrote ",
            "i edited",
            "edited ",
            "i updated",
            "updated ",
            "i implemented",
            "implemented ",
            "i ran",
            "ran ",
            "done,",
        ],
    )
}

pub(crate) fn model_first_provider_text_indicates_uncertainty(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    contains_any(
        &normalized,
        &[
            "i'm not sure",
            "i am not sure",
            "i don't know",
            "i do not know",
            "not sure which",
            "not sure what",
            "need clarification",
            "need guidance",
            "unclear",
            "ambiguous",
            "which folder",
            "which file",
            "which target",
        ],
    )
}

pub(crate) fn is_explicit_named_desktop_create_request(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    contains_any_word(normalized, &["create", "make", "mkdir"])
        && mentions_desktop_location(normalized)
        && (contains_any(
            normalized,
            &[
                " called ",
                " named ",
                " name it ",
                " call it ",
                " called \"",
                " named \"",
                " called '",
                " named '",
            ],
        ) || normalized.contains("desktop/"))
}

fn mentions_desktop_location(input: &str) -> bool {
    contains_any(
        input,
        &[
            "my desktop",
            "the desktop",
            "on desktop",
            "in desktop",
            "at desktop",
            "under desktop",
            "inside desktop",
            "desktop/",
        ],
    )
}

fn contains_any_word(value: &str, words: &[&str]) -> bool {
    value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| words.contains(&token))
}

pub(crate) fn should_block_model_first_auto_create_for_capability_question(
    input: &str,
    validated_actions: &[ValidatedModelToolAction],
) -> bool {
    is_capability_question_prompt(input)
        && validated_actions
            .iter()
            .any(|action| model_first_request_is_safe_create(&action.request))
}

fn is_capability_question_prompt(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    normalized.starts_with("can you ")
        || normalized.starts_with("could you ")
        || normalized.starts_with("would you ")
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        is_explicit_named_desktop_create_request, model_first_provider_text_indicates_uncertainty,
        policy_decision_for_model_first_action, should_ask_guidance_for_prose_only_model_first,
        should_block_model_first_auto_create_for_capability_question,
    };
    use crate::{
        action::{
            Action, ActionRequest, CreateDirectoryAction, CreateFileAction, DeleteFileAction,
        },
        model_runtime::ValidatedModelToolAction,
        policy::{PermissionPolicyMode, PolicyDecisionKind},
    };

    fn validated_action(request: ActionRequest) -> ValidatedModelToolAction {
        ValidatedModelToolAction {
            tool_call_id: "call-1".to_string(),
            target_label: request.approval_target(),
            request,
            summary: "test action".to_string(),
        }
    }

    #[test]
    fn model_first_decision_blocks_capability_question_safe_create() {
        let actions = vec![validated_action(ActionRequest::CreateDirectory(
            CreateDirectoryAction {
                target_path: PathBuf::from("demo"),
            },
        ))];

        assert!(
            should_block_model_first_auto_create_for_capability_question(
                "Can you create a folder called demo?",
                &actions
            )
        );
    }

    #[test]
    fn model_first_decision_allows_imperative_safe_create() {
        let actions = vec![validated_action(ActionRequest::CreateFile(
            CreateFileAction {
                target_path: PathBuf::from("demo.txt"),
                contents: "demo\n".to_string(),
            },
        ))];

        assert!(
            !should_block_model_first_auto_create_for_capability_question(
                "create a file called demo.txt",
                &actions
            )
        );
    }

    #[test]
    fn model_first_decision_uncertainty_detection_matches_guard_phrases() {
        assert!(model_first_provider_text_indicates_uncertainty(
            "I need clarification on which folder to use."
        ));
        assert!(model_first_provider_text_indicates_uncertainty(
            "The target is ambiguous."
        ));
        assert!(!model_first_provider_text_indicates_uncertainty(
            "I can create the requested file."
        ));
    }

    #[test]
    fn model_first_decision_recognizes_only_named_desktop_create_requests() {
        assert!(is_explicit_named_desktop_create_request(
            "Create a folder on my Desktop called ElgarLiveE2E"
        ));
        assert!(is_explicit_named_desktop_create_request(
            "create Desktop/ElgarLiveE2E"
        ));
        assert!(!is_explicit_named_desktop_create_request(
            "create a folder on my Desktop"
        ));
        assert!(!is_explicit_named_desktop_create_request(
            "create a folder there"
        ));
    }

    #[test]
    fn model_first_decision_prose_only_guidance_triggers_for_requests_and_claims() {
        assert!(should_ask_guidance_for_prose_only_model_first(
            "write a README",
            ""
        ));
        assert!(should_ask_guidance_for_prose_only_model_first(
            "tell me about this repo",
            "I created the README."
        ));
        assert!(!should_ask_guidance_for_prose_only_model_first(
            "tell me about this repo",
            "This repository contains Rust crates."
        ));
    }

    #[test]
    fn model_first_decision_policy_allows_only_safe_creates_in_auto_create_mode() {
        let create_action = Action::proposed_create_file("a1", "demo.txt", "demo\n", "create demo");
        let delete_action = Action::proposed(
            "a2",
            ActionRequest::DeleteFile(DeleteFileAction {
                target_path: PathBuf::from("demo.txt"),
            }),
            "delete demo",
        );

        let create_decision = policy_decision_for_model_first_action(
            PermissionPolicyMode::AutoCreateReviewModify,
            &create_action,
        );
        let delete_decision = policy_decision_for_model_first_action(
            PermissionPolicyMode::AutoCreateReviewModify,
            &delete_action,
        );
        let review_decision =
            policy_decision_for_model_first_action(PermissionPolicyMode::ReviewAll, &create_action);

        assert_eq!(create_decision.kind, PolicyDecisionKind::AllowApply);
        assert_eq!(delete_decision.kind, PolicyDecisionKind::RequireReview);
        assert_eq!(review_decision.kind, PolicyDecisionKind::RequireReview);
    }
}
