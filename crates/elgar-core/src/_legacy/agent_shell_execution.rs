use crate::{
    action::{ActionRequest, ShellCommandAction},
    agent_path_utils::absolute_session_path,
    agent_policy_flow::resolved_target_path_for_existing_check,
    agent_tool_output::ResolvedAgentToolOutput,
    session::Session,
};

pub(crate) fn guard_shell_execution_tool_outputs(
    session: &mut Session,
    outputs: Vec<ResolvedAgentToolOutput>,
    shell_execution: bool,
) -> Vec<ResolvedAgentToolOutput> {
    if !shell_execution {
        return outputs;
    }

    outputs
        .into_iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Action(mut action)
                if matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                if let ActionRequest::ShellCommand(shell) = action.request {
                    let (shell, dropped) =
                        drop_preexisting_shell_expected_paths(session, shell);
                    if dropped {
                        session.push_reasoning_runtime_check(
                            "ignored shell expected paths that already existed before execution",
                        );
                    }
                    action.request = ActionRequest::ShellCommand(shell);
                }
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action)
                if shell_execution_new_create_target_allowed(session, &action.request) =>
            {
                ResolvedAgentToolOutput::Action(action)
            }
            ResolvedAgentToolOutput::Action(action) => ResolvedAgentToolOutput::Skipped {
                tool_call_id: action.tool_call_id,
                message: "Skipped non-shell tool call because this execute intent requires shell_command or a new create target.".to_string(),
                visible: false,
            },
            other => other,
        })
        .collect()
}

fn drop_preexisting_shell_expected_paths(
    session: &Session,
    mut shell: ShellCommandAction,
) -> (ShellCommandAction, bool) {
    let mut dropped = false;

    if shell
        .expected_file
        .as_ref()
        .is_some_and(|path| absolute_session_path(session, path).is_file())
    {
        shell.expected_file = None;
        dropped = true;
    }

    let original_file_count = shell.expected_files.len();
    shell
        .expected_files
        .retain(|path| !absolute_session_path(session, path).is_file());
    dropped |= shell.expected_files.len() != original_file_count;

    if shell
        .expected_directory
        .as_ref()
        .is_some_and(|path| absolute_session_path(session, path).is_dir())
    {
        shell.expected_directory = None;
        dropped = true;
    }

    let original_directory_count = shell.expected_directories.len();
    shell
        .expected_directories
        .retain(|path| !absolute_session_path(session, path).is_dir());
    dropped |= shell.expected_directories.len() != original_directory_count;

    (shell, dropped)
}

fn shell_execution_new_create_target_allowed(session: &Session, request: &ActionRequest) -> bool {
    match request {
        ActionRequest::CreateFile(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::OverwriteFile(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::CreateDirectory(action) => {
            !resolved_target_path_for_existing_check(session, &action.target_path).exists()
        }
        ActionRequest::PatchFile(_)
        | ActionRequest::DeleteFile(_)
        | ActionRequest::MoveFile(_)
        | ActionRequest::ShellCommand(_) => false,
    }
}
