use crate::model_runtime::{ModelToolValidationError, ModelToolValidationErrorKind};

pub(crate) enum ToolValidationRecovery {
    RepairModel(String),
    Error(String),
}

pub(crate) fn tool_validation_recovery(error: &ModelToolValidationError) -> ToolValidationRecovery {
    if let Some(message) = tool_validation_repair_message(error) {
        return ToolValidationRecovery::RepairModel(message);
    }

    ToolValidationRecovery::Error(format!(
        "{} No filesystem action was applied.",
        friendly_tool_validation_error(error)
    ))
}

fn tool_validation_repair_message(error: &ModelToolValidationError) -> Option<String> {
    if !is_missing_or_malformed_tool_argument(error) {
        return None;
    }

    let tool = error.tool_name.as_deref().unwrap_or("tool");
    let repair_instruction = error
        .argument
        .as_deref()
        .map(|argument| format!("with `{argument}` included"))
        .unwrap_or_else(|| "with all required arguments included".to_string());
    Some(format!(
        "{} Use the original user request and verified session context to send a corrected `{tool}` tool call {repair_instruction}. No filesystem action was applied.",
        friendly_tool_validation_error(error)
    ))
}

fn is_missing_or_malformed_tool_argument(error: &ModelToolValidationError) -> bool {
    matches!(
        error.kind,
        ModelToolValidationErrorKind::MissingArgument
            | ModelToolValidationErrorKind::MalformedArgument
    )
}

fn friendly_tool_validation_error(error: &ModelToolValidationError) -> String {
    match (error.tool_name.as_deref(), error.argument.as_deref()) {
        (Some(tool), Some(argument)) => {
            format!("The `{tool}` tool call is incomplete or malformed for `{argument}`.")
        }
        (Some(tool), None) => format!("The `{tool}` tool call is incomplete or malformed."),
        (None, _) => "The model returned an incomplete or malformed tool call.".to_string(),
    }
}
