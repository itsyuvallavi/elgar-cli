use crate::{
    action::ActionRequest,
    model_runtime::{
        ValidatedModelGuidanceRequest, ValidatedModelToolAction, ValidatedModelToolOutput,
    },
    path_resolution::{resolve_agent_action_paths, AgentPathResolution},
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ResolvedAgentToolOutput {
    Guidance(ValidatedModelGuidanceRequest),
    Action(ValidatedModelToolAction),
    Skipped {
        tool_call_id: String,
        message: String,
        visible: bool,
    },
}

pub(crate) fn resolve_agent_tool_outputs(
    outputs: Vec<ValidatedModelToolOutput>,
    path_resolution: &AgentPathResolution,
) -> Vec<ResolvedAgentToolOutput> {
    outputs
        .into_iter()
        .map(|output| match output {
            ValidatedModelToolOutput::Guidance(guidance) => {
                ResolvedAgentToolOutput::Guidance(guidance)
            }
            ValidatedModelToolOutput::Action(action) => {
                ResolvedAgentToolOutput::Action(resolve_agent_action_paths(action, path_resolution))
            }
        })
        .collect()
}

pub(crate) fn resolved_outputs_are_shell_only(outputs: &[ResolvedAgentToolOutput]) -> bool {
    let mut saw_action = false;
    for output in outputs {
        match output {
            ResolvedAgentToolOutput::Action(action)
                if matches!(action.request, ActionRequest::ShellCommand(_)) =>
            {
                saw_action = true;
            }
            ResolvedAgentToolOutput::Action(_)
            | ResolvedAgentToolOutput::Guidance(_)
            | ResolvedAgentToolOutput::Skipped { .. } => return false,
        }
    }
    saw_action
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AllSkippedToolResultSignature {
    messages: Vec<String>,
}

pub(crate) fn all_skipped_tool_result_signature(
    outputs: &[ResolvedAgentToolOutput],
) -> Option<AllSkippedToolResultSignature> {
    if outputs.is_empty() {
        return None;
    }

    let mut messages = Vec::with_capacity(outputs.len());
    for output in outputs {
        let ResolvedAgentToolOutput::Skipped { message, .. } = output else {
            return None;
        };
        messages.push(message.clone());
    }

    Some(AllSkippedToolResultSignature { messages })
}

pub(crate) fn repeated_identical_skip_breaker_message(
    signature: &AllSkippedToolResultSignature,
) -> String {
    let mut messages = Vec::<&str>::new();
    for message in &signature.messages {
        if !messages.contains(&message.as_str()) {
            messages.push(message);
        }
    }

    format!(
        "Stopped because the model repeated the same blocked tool result without any verified action. Last block: {}",
        messages.join(" ")
    )
}

pub(crate) fn resolved_outputs_tool_call_ids(outputs: &[ResolvedAgentToolOutput]) -> Vec<String> {
    outputs
        .iter()
        .map(|output| match output {
            ResolvedAgentToolOutput::Guidance(guidance) => guidance.tool_call_id.clone(),
            ResolvedAgentToolOutput::Action(action) => action.tool_call_id.clone(),
            ResolvedAgentToolOutput::Skipped { tool_call_id, .. } => tool_call_id.clone(),
        })
        .collect()
}
