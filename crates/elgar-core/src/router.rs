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
    ApproveAction,
    RejectAction,
    Help,
    Unknown,
}

/// Classify user input without side effects.
pub fn route_input(input: &str) -> Route {
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

    if is_markdown_plan_file_request(&normalized) {
        return Route::ProposeMarkdownPlanFile;
    }

    if is_patch_file_request(&normalized) {
        return Route::ProposePatchFile;
    }

    if is_overwrite_file_request(&normalized) {
        return Route::ProposeOverwriteFile;
    }

    if is_delete_file_request(&normalized) {
        return Route::ProposeDeleteFile;
    }

    if is_move_file_request(&normalized) {
        return Route::ProposeMoveFile;
    }

    if is_create_directory_request(&normalized) {
        return Route::ProposeCreateDirectory;
    }

    if is_shell_command_request(&normalized) {
        return Route::ProposeShellCommand;
    }

    if is_write_file_request(&normalized) {
        return Route::ProposeWriteFile;
    }

    if is_model_question(&normalized) {
        return Route::AskModel;
    }

    if is_common_chat_text(&normalized) {
        return Route::AskModel;
    }

    Route::Unknown
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
        || input.starts_with("please create a directory ")
        || input.starts_with("please create a folder ")
        || input.starts_with("please make a directory ")
        || input.starts_with("please make a folder ")
        || input.starts_with("mkdir ")
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
        || input.contains(" plan to ")
        || input.contains(" with a plan")
        || input.contains(" markdown plan");
    let asks_to_create = input.starts_with("create ")
        || input.starts_with("write ")
        || input.starts_with("make ")
        || input.starts_with("draft ")
        || input.starts_with("can you create ")
        || input.starts_with("can you write ")
        || input.starts_with("please create ")
        || input.starts_with("please write ");

    asks_to_create && asks_for_file && asks_for_plan
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
        assert_eq!(route_input("create file plan.md"), Route::ProposeWriteFile);
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
            route_input("create a folder at /tmp/elgar-demo"),
            Route::ProposeCreateDirectory
        );
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
