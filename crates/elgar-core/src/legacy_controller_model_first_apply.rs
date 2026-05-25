use crate::{
    action::ActionRequest, model_runtime::ValidatedModelToolAction,
    provider_visible::provider_visible_text_from_text_only_output,
};

pub(crate) fn model_first_action_is_safe_create(action: &ValidatedModelToolAction) -> bool {
    model_first_request_is_safe_create(&action.request)
}

pub(crate) fn model_first_request_is_safe_create(request: &ActionRequest) -> bool {
    matches!(
        request,
        ActionRequest::CreateDirectory(_) | ActionRequest::CreateFile(_)
    )
}

pub(crate) fn model_first_no_tool_directory_fallback_would_truncate_compound_request(
    input: &str,
) -> bool {
    let normalized = input.to_ascii_lowercase();
    normalized.contains(", inside")
        || normalized.contains(" inside ")
        || normalized.contains(" project ")
        || normalized.contains(" app ")
}

pub(crate) fn model_first_no_tool_provider_text_should_remain_visible(provider_text: &str) -> bool {
    if provider_visible_text_from_text_only_output(provider_text.to_string()).is_none() {
        return false;
    }

    let normalized = provider_text.to_ascii_lowercase();
    !normalized.contains("tool")
        && !normalized.contains("target_path")
        && !normalized.contains("create_directory")
        && !normalized.contains("create_file")
        && !normalized.contains("call_create")
        && !normalized.contains("i created")
        && !normalized.contains("i wrote")
        && !normalized.contains("i implemented")
}
