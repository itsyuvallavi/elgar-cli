use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// A deterministic classification of user input.
///
/// Routes do not imply approval, application, provider output, or filesystem
/// truth. The router only classifies text for the controller to handle later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    AskModel,
    ProposeMarkdownPlanFile,
    ProposeWriteFile,
    ProposePatchFile,
    ProposeOverwriteFile,
    ProposeDeleteFile,
    ProposeMoveFile,
    ProposeCreateDirectory,
    ProposeShellCommand,
    ExecutePlan,
    ApproveAction,
    RejectAction,
    Help,
    Unknown,
}

/// Classify user input without side effects.
pub fn route_input(input: &str) -> Route {
    let input = normalize_pasted_transcript_input(input);
    let normalized = input.trim().to_ascii_lowercase();

    if normalized.is_empty() {
        return Route::Unknown;
    }

    match normalized.as_str() {
        "help" | "/help" | "--help" | "-h" | "?" => return Route::Help,
        "approve" | "approved" | "accept" | "accepted" | "yes" | "y" | "ok" => {
            return Route::ApproveAction;
        }
        "reject" | "rejected" | "deny" | "denied" | "no" | "n" | "cancel" => {
            return Route::RejectAction;
        }
        _ => {}
    }

    let action_input = strip_action_request_prefixes(&normalized);

    if is_execute_plan_request(action_input) {
        return Route::ExecutePlan;
    }

    if is_patch_file_request(action_input) {
        return Route::ProposePatchFile;
    }

    if is_overwrite_file_request(action_input) {
        return Route::ProposeOverwriteFile;
    }

    if is_delete_file_request(action_input) {
        return Route::ProposeDeleteFile;
    }

    if is_move_file_request(action_input) {
        return Route::ProposeMoveFile;
    }

    if is_create_directory_request(action_input) {
        return Route::ProposeCreateDirectory;
    }

    if is_shell_command_request(action_input) {
        return Route::ProposeShellCommand;
    }

    if is_write_file_request(action_input) {
        return Route::ProposeWriteFile;
    }

    if is_project_creation_request(action_input) || is_markdown_plan_file_request(action_input) {
        return Route::ProposeMarkdownPlanFile;
    }

    if is_model_question(&normalized) {
        return Route::AskModel;
    }

    if is_common_chat_text(&normalized) {
        return Route::AskModel;
    }

    Route::Unknown
}

pub(crate) fn strip_action_request_prefixes(input: &str) -> &str {
    let mut stripped = input.trim_start();

    loop {
        let next = stripped
            .strip_prefix("can you ")
            .or_else(|| stripped.strip_prefix("could you "))
            .or_else(|| stripped.strip_prefix("would you "))
            .or_else(|| stripped.strip_prefix("please "))
            .or_else(|| stripped.strip_prefix("okay "))
            .or_else(|| stripped.strip_prefix("ok "));

        match next {
            Some(value) => stripped = value.trim_start(),
            None => return stripped,
        }
    }
}

/// Strip prompt text copied from Elgar transcripts before deterministic routing.
///
/// This keeps provider output unchanged while letting pasted user prompt lines
/// such as `> create ...` and `> > create ...` reach the controller path.
pub fn normalize_pasted_transcript_input(input: &str) -> Cow<'_, str> {
    let stripped = strip_pasted_transcript_markers(input);
    if stripped.len() == input.len() {
        Cow::Borrowed(input)
    } else {
        Cow::Owned(stripped.to_string())
    }
}

fn strip_pasted_transcript_markers(input: &str) -> &str {
    let mut stripped = input.trim_start();

    loop {
        let before = stripped;

        while let Some(rest) = stripped.strip_prefix('>') {
            stripped = rest.trim_start();
        }

        for prefix in ["user:", "you:", "human:", "me:"] {
            if let Some(rest) = strip_ascii_case_prefix(stripped, prefix) {
                stripped = rest.trim_start();
                break;
            }
        }

        if stripped == before {
            return stripped;
        }
    }
}

fn strip_ascii_case_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let head = input.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &input[prefix.len()..])
}

fn is_patch_file_request(input: &str) -> bool {
    (input.starts_with("edit file ")
        || input.starts_with("edit ")
        || input.starts_with("patch file ")
        || input.starts_with("patch "))
        && input.contains(" replace ")
        && input.contains(" with ")
}

fn is_overwrite_file_request(input: &str) -> bool {
    (input.starts_with("overwrite file ") || input.starts_with("overwrite "))
        && input.contains(" with ")
}

fn is_delete_file_request(input: &str) -> bool {
    input.starts_with("delete file ")
        || input.starts_with("delete ")
        || input.starts_with("remove file ")
        || input.starts_with("remove ")
}

fn is_move_file_request(input: &str) -> bool {
    (input.starts_with("move file ")
        || input.starts_with("move ")
        || input.starts_with("rename file ")
        || input.starts_with("rename "))
        && input.contains(" to ")
}

fn is_create_directory_request(input: &str) -> bool {
    input.starts_with("create directory ")
        || input.starts_with("create a directory ")
        || input.starts_with("create dir ")
        || input.starts_with("create folder ")
        || input.starts_with("create a folder ")
        || input.starts_with("make directory ")
        || input.starts_with("make a directory ")
        || input.starts_with("make dir ")
        || input.starts_with("make folder ")
        || input.starts_with("make a folder ")
        || input.starts_with("can you create a directory ")
        || input.starts_with("can you create a folder ")
        || input.starts_with("can you make a directory ")
        || input.starts_with("can you make a folder ")
        || input.starts_with("i want you to create a directory ")
        || input.starts_with("i want you to create a folder ")
        || input.starts_with("i want you to make a directory ")
        || input.starts_with("i want you to make a folder ")
        || input.starts_with("please create a directory ")
        || input.starts_with("please create a folder ")
        || input.starts_with("please make a directory ")
        || input.starts_with("please make a folder ")
        || input.starts_with("mkdir ")
        || is_create_directory_plan_followup(input)
}

fn is_create_directory_plan_followup(input: &str) -> bool {
    let asks_to_create =
        input.contains("create ") || input.contains("make ") || input.contains("generate ");
    let references_prior_plan = input.contains("this plan")
        || input.contains("that plan")
        || input.contains("the plan")
        || input.contains("these folders")
        || input.contains("those folders")
        || input.contains("the folders");
    let has_location_or_folder_intent = input.contains("desktop")
        || input.contains("folder")
        || input.contains("directory")
        || input.contains(" at ")
        || input.contains(" in ")
        || input.contains(" under ")
        || input.contains(" inside ");

    asks_to_create && references_prior_plan && has_location_or_folder_intent
}

fn is_shell_command_request(input: &str) -> bool {
    input.starts_with("run ")
        || input.starts_with("run command ")
        || input.starts_with("run shell command ")
        || input.starts_with("run shell ")
        || input.starts_with("shell command ")
}

fn is_write_file_request(input: &str) -> bool {
    input.starts_with("create file ")
        || input.starts_with("create a file ")
        || input.starts_with("write file ")
        || input.starts_with("write a file ")
        || starts_with_file_target(input, "create ")
        || starts_with_file_target(input, "write ")
}

fn is_markdown_plan_file_request(input: &str) -> bool {
    let asks_for_file = input.contains(" md file")
        || input.contains(" markdown file")
        || input.contains(" markdown document")
        || input.contains(" markdown plan")
        || input.contains(".md");
    let asks_for_plan = input.contains(" plan ")
        || input.starts_with("create a plan ")
        || input.starts_with("create plan ")
        || input.starts_with("please create a plan ")
        || input.starts_with("please write a plan ")
        || input.contains(" plan to ")
        || input.contains(" with a plan")
        || input.contains(" markdown plan");
    let asks_for_local_plan_artifact = asks_for_plan
        && (input.contains(" desktop")
            || input.contains("~/")
            || input.contains("$home/")
            || input.contains(" on ")
            || input.contains(" in ")
            || input.contains(" at ")
            || input.contains(" under ")
            || input.contains(" inside "));
    let asks_to_create = input.starts_with("create ")
        || input.starts_with("write ")
        || input.starts_with("make ")
        || input.starts_with("draft ")
        || input.starts_with("can you create ")
        || input.starts_with("can you write ")
        || input.starts_with("please create ")
        || input.starts_with("please write ");

    asks_to_create && asks_for_plan && (asks_for_file || asks_for_local_plan_artifact)
}

pub(crate) fn is_project_creation_request(input: &str) -> bool {
    let input = input.trim().to_ascii_lowercase();
    let input = strip_action_request_prefixes(&input);
    let asks_to_create = input.starts_with("create ")
        || input.starts_with("make ")
        || input.starts_with("build ")
        || input.starts_with("scaffold ")
        || input.starts_with("generate ")
        || input.starts_with("can you create ")
        || input.starts_with("can you make ")
        || input.starts_with("can you build ")
        || input.starts_with("please create ")
        || input.starts_with("please make ")
        || input.starts_with("please build ");
    let explicitly_asks_for_plan = input.contains(" plan ")
        || input.starts_with("create a plan ")
        || input.starts_with("create plan ")
        || input.starts_with("please create a plan ");

    asks_to_create && contains_project_creation_subject(&input) && !explicitly_asks_for_plan
}

fn contains_project_creation_subject(input: &str) -> bool {
    input.split_whitespace().any(|word| {
        let word = word.trim_matches(|character: char| !character.is_ascii_alphanumeric());
        matches!(
            word,
            "project" | "projects" | "app" | "apps" | "application" | "applications" | "react"
        )
    })
}

fn is_execute_plan_request(input: &str) -> bool {
    let input = input
        .strip_prefix("okay ")
        .or_else(|| input.strip_prefix("ok "))
        .or_else(|| input.strip_prefix("please "))
        .unwrap_or(input);

    let references_plan = input.contains("the plan")
        || input.contains("this plan")
        || input.contains("that plan")
        || input.contains("the proposed plan");
    let asks_to_execute = input.starts_with("execute ")
        || input.starts_with("apply ")
        || input.starts_with("run ")
        || input.starts_with("build ")
        || input.contains(" execute ")
        || input.contains(" apply ")
        || input.contains(" run ")
        || input.contains(" build ")
        || input.contains(" according to the plan");

    (references_plan && asks_to_execute) || is_prior_project_execution_request(input)
}

pub(crate) fn is_prior_project_execution_request(input: &str) -> bool {
    let lowered = input.trim().to_ascii_lowercase();
    let input = lowered.as_str();
    let input = trim_trailing_sentence_punctuation(input);
    let input = strip_action_request_prefixes(input);
    let input = trim_trailing_sentence_punctuation(input);
    let normalized = input;
    let asks_to_create = normalized.starts_with("create ")
        || normalized.starts_with("make ")
        || normalized.starts_with("build ")
        || normalized.starts_with("scaffold ")
        || normalized.starts_with("generate ");
    let references_project = matches!(
        normalized,
        "create the project"
            | "make the project"
            | "build the project"
            | "scaffold the project"
            | "generate the project"
            | "create the project you planned"
            | "make the project you planned"
            | "build the project you planned"
            | "create the project we planned"
            | "create the project from the plan"
            | "create this project"
            | "make this project"
            | "build this project"
            | "create that project"
            | "make that project"
            | "build that project"
    );

    asks_to_create && references_project
}

fn trim_trailing_sentence_punctuation(input: &str) -> &str {
    input.trim_end_matches(|character: char| {
        character.is_ascii_whitespace() || matches!(character, '.' | '!' | '?')
    })
}

fn starts_with_file_target(input: &str, prefix: &str) -> bool {
    input
        .strip_prefix(prefix)
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|target| target.contains('.') || target.contains('/'))
}

fn is_model_question(input: &str) -> bool {
    input.contains('?')
        || input.starts_with("explain ")
        || input.starts_with("what ")
        || input.starts_with("why ")
        || input.starts_with("how ")
        || input.starts_with("where ")
        || input.starts_with("when ")
        || input.starts_with("who ")
        || input.starts_with("can you ")
        || input.starts_with("tell me ")
        || input.starts_with("say ")
}

fn is_common_chat_text(input: &str) -> bool {
    let chat = input.trim_matches(|character: char| {
        character.is_ascii_whitespace() || matches!(character, '.' | '!' | '?')
    });

    matches!(
        chat,
        "hello" | "hi" | "hey" | "hello elgar" | "hi elgar" | "hey elgar"
    )
}

#[cfg(test)]
mod tests {
    use super::{route_input, Route};

    #[test]
    fn classifies_help_input() {
        assert_eq!(route_input("help"), Route::Help);
        assert_eq!(route_input("--help"), Route::Help);
    }

    #[test]
    fn classifies_write_file_requests() {
        assert_eq!(route_input("create hello.py"), Route::ProposeWriteFile);
        assert_eq!(route_input("write file notes.txt"), Route::ProposeWriteFile);
    }

    #[test]
    fn classifies_markdown_plan_file_requests() {
        assert_eq!(
            route_input("create an md file with a plan to create a calculator UI using python"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("please write a markdown plan for a small CLI"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("please create a plan for a simple project on my desktop"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(route_input("create file plan.md"), Route::ProposeWriteFile);
    }

    #[test]
    fn classifies_natural_project_creation_as_controller_owned_plan_request() {
        assert_eq!(
            route_input(
                "can you create a project on the desktop inside a folder you need to create called Demo? the project should be a simple react TS project."
            ),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("can you please create a react project called demo"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("can you create a simple React TS project at /tmp/demo?"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("create a React TS app in Demo"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("create a project called Demo"),
            Route::ProposeMarkdownPlanFile
        );
    }

    #[test]
    fn explicit_file_creation_wins_over_natural_project_terms() {
        assert_eq!(
            route_input("create file approved.py"),
            Route::ProposeWriteFile
        );
        assert_eq!(
            route_input("create file applied.py"),
            Route::ProposeWriteFile
        );
        assert_eq!(route_input("create file app.py"), Route::ProposeWriteFile);
        assert_eq!(route_input("create app.py"), Route::ProposeWriteFile);
        assert_eq!(
            route_input("please write hidden-plan.md with a markdown plan for hidden work"),
            Route::ProposeWriteFile
        );
    }

    #[test]
    fn classifies_execute_plan_followups_without_provider_chat() {
        assert_eq!(route_input("okay execute the plan"), Route::ExecutePlan);
        assert_eq!(route_input("apply this plan"), Route::ExecutePlan);
        assert_eq!(route_input("yes execute the plan!"), Route::ExecutePlan);
        assert_eq!(route_input("now execute the plan"), Route::ExecutePlan);
        assert_eq!(
            route_input("create the project according to the plan!"),
            Route::ExecutePlan
        );
        assert_eq!(route_input("create the project"), Route::ExecutePlan);
        assert_eq!(
            route_input("create the project you planned"),
            Route::ExecutePlan
        );
    }

    #[test]
    fn classifies_patch_and_overwrite_file_requests() {
        assert_eq!(
            route_input("edit file notes.txt replace old with new"),
            Route::ProposePatchFile
        );
        assert_eq!(
            route_input("patch notes.txt replace old with new"),
            Route::ProposePatchFile
        );
        assert_eq!(
            route_input("overwrite file notes.txt with new contents"),
            Route::ProposeOverwriteFile
        );
    }

    #[test]
    fn classifies_shell_command_requests() {
        assert_eq!(
            route_input("run command cargo test -p elgar-core"),
            Route::ProposeShellCommand
        );
        assert_eq!(
            route_input("run shell echo hello"),
            Route::ProposeShellCommand
        );
        assert_eq!(route_input("shell command pwd"), Route::ProposeShellCommand);
        assert_eq!(route_input("run ls"), Route::ProposeShellCommand);
    }

    #[test]
    fn classifies_natural_folder_requests_as_directory_actions() {
        assert_eq!(
            route_input("create a folder called hello-world"),
            Route::ProposeCreateDirectory
        );
        assert_eq!(
            route_input("can you create a folder called hello-world in the desktop?"),
            Route::ProposeCreateDirectory
        );
        assert_eq!(
            route_input("can you please create a folder called review-guard"),
            Route::ProposeCreateDirectory
        );
        assert_eq!(
            route_input("create a folder at /tmp/elgar-demo"),
            Route::ProposeCreateDirectory
        );
        assert_eq!(
            route_input("okay create this plan on my desktop"),
            Route::ProposeCreateDirectory
        );
    }

    #[test]
    fn strips_pasted_transcript_prompt_markers_before_routing_actions() {
        assert_eq!(
            route_input("> create a folder called hello-world"),
            Route::ProposeCreateDirectory
        );
        assert_eq!(
            route_input("> > create a plan for a basic react project inside that folder"),
            Route::ProposeMarkdownPlanFile
        );
        assert_eq!(
            route_input("User: write file notes.txt"),
            Route::ProposeWriteFile
        );
        assert_eq!(route_input("> create the project"), Route::ExecutePlan);
    }

    #[test]
    fn classifies_approval_input() {
        assert_eq!(route_input("approve"), Route::ApproveAction);
        assert_eq!(route_input("yes"), Route::ApproveAction);
    }

    #[test]
    fn classifies_rejection_input() {
        assert_eq!(route_input("reject"), Route::RejectAction);
        assert_eq!(route_input("no"), Route::RejectAction);
    }

    #[test]
    fn classifies_model_questions() {
        assert_eq!(route_input("explain this function"), Route::AskModel);
        assert_eq!(route_input("what does this code do?"), Route::AskModel);
        assert_eq!(
            route_input("can you explain what you can do?"),
            Route::AskModel
        );
        assert_eq!(route_input("Say hello in one sentence."), Route::AskModel);
    }

    #[test]
    fn classifies_common_chat_greetings_as_model_input() {
        assert_eq!(route_input("hello!"), Route::AskModel);
        assert_eq!(route_input("Hi Elgar."), Route::AskModel);
    }

    #[test]
    fn unknown_and_empty_input_fail_safely() {
        assert_eq!(route_input(""), Route::Unknown);
        assert_eq!(route_input("   "), Route::Unknown);
        assert_eq!(route_input("create a plan"), Route::Unknown);
        assert_eq!(route_input("bash -lc ls"), Route::Unknown);
    }
}
