use crate::{
    agent_turn_router::AgentExecutionIntent,
    model_runtime::{
        elgar_model_tool_definitions, elgar_model_tool_definitions_for, ModelToolName,
        RawModelToolCall,
    },
    provider::ChatToolDefinition,
};

pub(crate) fn tool_definitions_for_intent(intent: AgentExecutionIntent) -> Vec<ChatToolDefinition> {
    if intent.explicit_tool_command {
        return elgar_model_tool_definitions();
    }
    if intent.shell_execution {
        return elgar_model_tool_definitions_for(&[
            ModelToolName::AskGuidance,
            ModelToolName::ShellCommand,
        ]);
    }
    if intent.plan_execution || intent.plan_creation_execution {
        return elgar_model_tool_definitions_for(&[
            ModelToolName::AskGuidance,
            ModelToolName::CreateFiles,
            ModelToolName::CreateFile,
            ModelToolName::CreateDirectory,
            ModelToolName::OverwriteFile,
            ModelToolName::PatchFile,
            ModelToolName::ShellCommand,
        ]);
    }
    elgar_model_tool_definitions()
}

pub(crate) fn validate_tool_calls_in_scope(
    tool_calls: &[RawModelToolCall],
    tools: &[ChatToolDefinition],
) -> Result<(), String> {
    let allowed_names = tools
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect::<Vec<_>>();
    for tool_call in tool_calls {
        let tool_name = tool_call.name.raw_label();
        if !allowed_names.contains(&tool_name.as_str()) {
            let allowed = allowed_names.join(", ");
            return Err(format!(
                "Tool `{tool_name}` is not available for this route. Use one of: {allowed}."
            ));
        }
    }
    Ok(())
}
