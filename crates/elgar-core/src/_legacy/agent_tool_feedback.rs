use crate::{
    action::ActionRequest, model_runtime::ValidatedModelToolOutput, provider::ChatMessage,
    session::Session, shell_allowlist::is_read_only_shell_command,
};

const TOOL_COMMAND_PREFIX: &str = "/tool";

pub(crate) fn explicit_tool_command_instruction() -> &'static str {
    "Explicit tool command: carry out the requested action directly. If the target file has been read and the requested edit is concrete, use overwrite_file or patch_file next; do not re-read the same file or list unrelated files. After a successful edit, run the requested verification command if one was requested."
}

pub(crate) fn explicit_tool_repeated_inspection_feedback() -> String {
    "Skipped read-only inspection because this explicit tool command already has the needed context. Call overwrite_file or patch_file now for the requested edit; use shell_command only for verification after the edit."
        .to_string()
}

pub(crate) fn explicit_tool_read_only_stall_limit_message() -> String {
    "Stopped explicit tool command because the model kept returning read-only shell commands after the needed context was already available. No edit was applied; retry with a more direct edit request or specify the exact file contents."
        .to_string()
}

pub(crate) fn explicit_tool_completed_shell_feedback(result: String) -> String {
    format!(
        "{result}\nThe requested shell command has completed. Use the tool result already in context and answer the user in normal prose now. Do not call another tool unless the user explicitly requested another command or the result is insufficient."
    )
}

pub(crate) fn repeated_shell_command_feedback(explicit_tool_command: bool) -> String {
    if explicit_tool_command {
        "Skipped repeated shell command because the same command already completed in this turn. Use the earlier tool result already in context; do not run the same read command again. Continue with the requested edit, verification command, or final answer."
            .to_string()
    } else {
        "Skipped repeated shell command because the same command already completed in this turn."
            .to_string()
    }
}

pub(crate) fn explicit_tool_command_input(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    if trimmed == TOOL_COMMAND_PREFIX {
        return Some("");
    }

    let rest = trimmed.strip_prefix(TOOL_COMMAND_PREFIX)?;
    rest.strip_prefix(' ')
        .or_else(|| rest.strip_prefix('\t'))
        .map(str::trim)
}

pub(crate) fn is_read_only_shell_action(request: &ActionRequest) -> bool {
    match request {
        ActionRequest::ShellCommand(shell) => is_read_only_shell_command(shell),
        _ => false,
    }
}

pub(crate) fn record_validated_tool_output_trace(
    session: &mut Session,
    outputs: &[ValidatedModelToolOutput],
) {
    for output in outputs {
        match output {
            ValidatedModelToolOutput::Action(action) => {
                session.push_reasoning_model_decision(format!(
                    "requested {}: {}",
                    action_request_kind_label(&action.request),
                    action.request.approval_target()
                ));
            }
            ValidatedModelToolOutput::Guidance(guidance) => {
                session.push_reasoning_model_decision(format!(
                    "requested guidance: {}",
                    guidance.question
                ));
            }
        }
    }
}

pub(crate) fn append_tool_feedback_message(
    messages: &mut Vec<ChatMessage>,
    tool_call_id: String,
    content: String,
) {
    if let Some(existing) = messages
        .iter_mut()
        .rev()
        .find(|message| message.tool_call_id.as_deref() == Some(tool_call_id.as_str()))
    {
        if !existing.content.is_empty() {
            existing.content.push('\n');
        }
        existing.content.push_str(&content);
        return;
    }

    messages.push(ChatMessage::tool(tool_call_id, content));
}

fn action_request_kind_label(request: &ActionRequest) -> &'static str {
    match request {
        ActionRequest::CreateFile(_) => "create_file",
        ActionRequest::CreateDirectory(_) => "create_directory",
        ActionRequest::OverwriteFile(_) => "overwrite_file",
        ActionRequest::PatchFile(_) => "patch_file",
        ActionRequest::DeleteFile(_) => "delete_file",
        ActionRequest::MoveFile(_) => "move_file",
        ActionRequest::ShellCommand(_) => "shell_command",
    }
}

pub(crate) fn shell_command_signature(request: &ActionRequest) -> Option<String> {
    let ActionRequest::ShellCommand(shell) = request else {
        return None;
    };
    Some(format!("{}\n{}", shell.cwd.display(), shell.command.trim()))
}
