use serde::{Deserialize, Serialize};

/// A deterministic classification of user input.
///
/// Routes do not imply approval, application, provider output, or filesystem
/// truth. The router only classifies text for the controller to handle later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    AskModel,
    ProposeWriteFile,
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

    if is_write_file_request(&normalized) {
        return Route::ProposeWriteFile;
    }

    if is_model_question(&normalized) {
        return Route::AskModel;
    }

    Route::Unknown
}

fn is_write_file_request(input: &str) -> bool {
    input.starts_with("create file ")
        || input.starts_with("create a file ")
        || input.starts_with("write file ")
        || input.starts_with("write a file ")
        || starts_with_file_target(input, "create ")
        || starts_with_file_target(input, "write ")
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
    }

    #[test]
    fn unknown_and_empty_input_fail_safely() {
        assert_eq!(route_input(""), Route::Unknown);
        assert_eq!(route_input("   "), Route::Unknown);
        assert_eq!(route_input("create a plan"), Route::Unknown);
        assert_eq!(route_input("run ls"), Route::Unknown);
    }
}
