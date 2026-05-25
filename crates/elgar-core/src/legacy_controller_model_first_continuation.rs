use std::path::PathBuf;

use crate::{
    controller_reporting::truth_guard_visible_message,
    event::{AssistantMessage, AssistantMessageSource, Event},
    legacy_controller_model_first_plan_completion::ModelFirstPlanCompletenessNeed,
    model_runtime::{elgar_model_tool_definitions, ValidatedModelToolAction},
    provider::ChatToolDefinition,
    session::Session,
};

const MODEL_FIRST_TOOL_CONTRACT: &str = "Model-first tool contract selected by Elgar controller:\n- For requests to create, implement, or make project files, return create_directory/create_file tool calls for actual filesystem changes; do not answer with prose-only file contents or claim success.\n- If the user explicitly names Desktop and gives a folder or file name, that target is clear. Use create_directory/create_file; do not ask whether Desktop means the user's home Desktop directory.\n- If target, scope, verified memory, or safe next step is ambiguous, use ask_guidance with one concise question instead of guessing.\n- Multiple safe create_file/create_directory calls are allowed for multi-file project creation.\n- Shell, overwrite, patch, delete, and move are review-gated. Do not use shell commands for package installation or project setup in this flow.\n- When verified memory names a latest folder, same folder, or plan project root, target project files inside that verified folder/root.\n- When verified memory includes a latest verified plan content excerpt, use that excerpt as the plan source; do not ask what the plan contains.";

const MODEL_FIRST_COMPLETENESS_CONTINUATION_CONTRACT: &str = "Model-first implementation completeness follow-up selected by Elgar controller:\n- Return tool calls now.\n- Required: create_file tool calls for every missing project file listed below.\n- Allowed: create_directory only if required for a missing file parent directory.\n- Forbidden: prose-only answers, markdown-only file listings, success claims, shell commands, package installation, overwrite, patch, delete, and move tools.\n- Filesystem truth will be recorded only after a complete safe create batch validates.";

#[derive(Debug)]
pub(crate) enum ModelFirstCompletenessContinuation {
    NotNeeded,
    ContinueWith(Vec<ValidatedModelToolAction>),
    Blocked,
}

#[derive(Debug)]
pub(crate) enum ModelFirstCompletenessContinuationAttempt {
    NoToolCalls,
    Incomplete,
    Done(ModelFirstCompletenessContinuation),
}

pub(crate) fn model_first_completeness_continuation_prompt(
    input: &str,
    provider_text: &str,
    need: &ModelFirstPlanCompletenessNeed,
) -> String {
    let expected_files = display_path_list(&need.expected_files);
    let missing_files = display_path_list(&need.missing_files);
    let provider_text = compact_prompt_text(provider_text);
    format!(
        "{MODEL_FIRST_TOOL_CONTRACT}\n\n{MODEL_FIRST_COMPLETENESS_CONTINUATION_CONTRACT}\n\nReturn create_file tool calls for these exact missing files: {missing_files}\nVerified plan: {}\nProject root: {}\nExpected files: {expected_files}\nVerified plan content excerpt: {}\nOriginal user request: {}\nPrior provider text: {}",
        need.plan_path.display(),
        need.project_root.display(),
        need.plan_excerpt,
        input.trim(),
        provider_text,
    )
}

pub(crate) fn model_first_final_completeness_continuation_prompt(
    input: &str,
    need: &ModelFirstPlanCompletenessNeed,
) -> String {
    let missing_files = display_path_list(&need.missing_files);
    format!(
        "FINAL TOOL-CALL RETRY.\nReturn only create_file tool calls.\nRequired target_path values: {missing_files}\nNo prose. No markdown. No explanations. No success claim. No shell. No package installation. No overwrite, patch, delete, move, or guidance.\nProject root: {}\nVerified plan: {}\nPlan excerpt: {}\nUser request: {}",
        need.project_root.display(),
        need.plan_path.display(),
        need.plan_excerpt,
        input.trim(),
    )
}

pub(crate) fn model_first_create_file_tool_definitions() -> Vec<ChatToolDefinition> {
    elgar_model_tool_definitions()
        .into_iter()
        .filter(|tool| tool.function.name == "create_file")
        .collect()
}

pub(crate) fn push_model_first_incomplete_plan_message(
    session: &mut Session,
    need: &ModelFirstPlanCompletenessNeed,
) {
    let missing_files = display_path_list(&need.missing_files);
    push_controller_message(
        session,
        format!(
            "I only received partial implementation tool calls. No files were changed because the verified plan still needs create_file calls for: {missing_files}."
        ),
    );
}

fn push_controller_message(session: &mut Session, message: impl Into<String>) {
    let message = truth_guard_visible_message(session, message.into());
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
}

fn display_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn compact_prompt_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
