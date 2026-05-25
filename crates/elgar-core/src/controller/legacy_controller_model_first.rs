//! Legacy controller-review model-first runtime.
//!
//! Normal live TUI turns use `agent_loop::run_permissive_agent_turn`. This
//! module remains only for compatibility and controller-review smoke coverage.

use std::path::{Path, PathBuf};

use super::{
    build_project_scaffold_plan, controller_owned_verified_plan_scaffold_actions,
    first_existing_scaffold_target, home_dir, next_action_id, parse_create_directory_target,
    push_controller_message, Controller, ParsedCreateDirectoryTarget, TurnResult,
};
use crate::{
    action::{Action, ActionRequest, CreateDirectoryAction},
    controller_prompt::{model_first_provider_prompt_with_context, VerifiedMemoryNeed},
    controller_provider::{
        push_provider_message_if_visible, record_provider_request_metadata,
        set_provider_metrics_metadata,
    },
    event::{ActionEvent, ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage},
    followup_action_paths::{
        explicit_request_base, followup_base_path_for_request,
        retarget_safe_create_to_followup_base,
    },
    legacy_controller_model_first_apply::{
        model_first_action_is_safe_create,
        model_first_no_tool_directory_fallback_would_truncate_compound_request,
        model_first_no_tool_provider_text_should_remain_visible,
    },
    legacy_controller_model_first_continuation::{
        model_first_completeness_continuation_prompt, model_first_create_file_tool_definitions,
        model_first_final_completeness_continuation_prompt,
        push_model_first_incomplete_plan_message, ModelFirstCompletenessContinuation,
        ModelFirstCompletenessContinuationAttempt,
    },
    legacy_controller_model_first_decision::{
        is_explicit_named_desktop_create_request, model_first_proposal_message,
        model_first_provider_text_indicates_uncertainty, policy_decision_for_model_first_action,
        should_ask_guidance_for_prose_only_model_first,
        should_block_model_first_auto_create_for_capability_question,
    },
    legacy_controller_model_first_plan_completion::{
        missing_expected_verified_plan_files, model_first_verified_plan_completion_need,
        ModelFirstPlanCompletenessNeed,
    },
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ValidatedModelGuidanceRequest,
        ValidatedModelToolAction, ValidatedModelToolOutput,
    },
    policy::{PermissionPolicyMode, PolicyDecision, PolicyDecisionKind},
    provider::ControllerProvider,
    router::{normalize_pasted_transcript_input, route_input, Route},
    session::{ActionRecord, PendingActionSelection, Session},
};

impl<P> Controller<P>
where
    P: ControllerProvider,
{
    /// Record a legacy controller-review model-first turn.
    ///
    /// This path is retained for explicit controller smoke/review tests. It is
    /// no longer the live TUI runtime; normal TUI turns use `agent_loop`.
    /// The provider can suggest text and tool calls, but the controller still
    /// validates each draft and may review-gate it according to `mode`.
    pub fn legacy_controller_model_first_turn_with_policy(
        &self,
        session: &mut Session,
        input: &str,
        mode: PermissionPolicyMode,
    ) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));
        self.handle_legacy_controller_model_first_turn_with_policy(session, input, mode);

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    fn handle_legacy_controller_model_first_turn_with_policy(
        &self,
        session: &mut Session,
        input: &str,
        mode: PermissionPolicyMode,
    ) {
        let normalized_input = normalize_pasted_transcript_input(input);
        let controller_input = normalized_input.as_ref();
        if self.handle_legacy_controller_model_first_escape_hatch(session, controller_input, mode) {
            return;
        }

        let request = self.provider.request_metadata();
        record_provider_request_metadata(session, &request);

        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        let provider_prompt = model_first_provider_prompt_with_context(session, controller_input);
        match self.provider.chat_with_tools_with_metadata(
            &provider_prompt,
            &request,
            elgar_model_tool_definitions(),
        ) {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                let provider_text = output.text.clone();
                let tool_calls = output.tool_calls.clone();
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider,
                    request.request_id,
                    output,
                )));

                match validate_model_tool_outputs(&tool_calls) {
                    Ok(outputs) => {
                        self.handle_validated_legacy_controller_model_first_outputs(
                            session,
                            controller_input,
                            &provider_text,
                            outputs,
                            mode,
                        );
                    }
                    Err(error) => {
                        session.push_event(Event::Error(ErrorEvent::new(error.message)));
                    }
                }
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
            }
        }
    }

    fn handle_legacy_controller_model_first_escape_hatch(
        &self,
        _session: &mut Session,
        _input: &str,
        _mode: PermissionPolicyMode,
    ) -> bool {
        false
    }

    fn handle_validated_legacy_controller_model_first_outputs(
        &self,
        session: &mut Session,
        input: &str,
        provider_text: &str,
        validated_outputs: Vec<ValidatedModelToolOutput>,
        mode: PermissionPolicyMode,
    ) {
        let mut validated_actions = Vec::new();
        let mut guidance_requests = Vec::new();
        for output in validated_outputs {
            match output {
                ValidatedModelToolOutput::Action(action) => validated_actions.push(action),
                ValidatedModelToolOutput::Guidance(guidance) => guidance_requests.push(guidance),
            }
        }

        let explicit_named_desktop_create = is_explicit_named_desktop_create_request(input);
        let explicit_named_desktop_safe_create = explicit_named_desktop_create
            && !validated_actions.is_empty()
            && validated_actions
                .iter()
                .all(model_first_action_is_safe_create);

        if validated_actions.is_empty()
            && self.try_apply_legacy_controller_model_first_directory_fallback(
                session,
                input,
                provider_text,
                mode,
            )
        {
            return;
        }

        if let Some(guidance) = guidance_requests.first() {
            if explicit_named_desktop_create {
                // The explicit Desktop target is actionable; keep the provider's
                // clarification request out of the user transcript.
            } else {
                push_model_first_guidance_message(session, guidance);
                return;
            }
        }

        if validated_actions.is_empty() {
            if self.try_apply_legacy_controller_model_first_directory_fallback(
                session,
                input,
                provider_text,
                mode,
            ) {
                return;
            }

            if model_first_no_tool_provider_text_should_remain_visible(provider_text) {
                push_provider_message_if_visible(session, provider_text.to_string());
            } else if should_ask_guidance_for_prose_only_model_first(input, provider_text) {
                push_controller_message(
                    session,
                    "I did not receive a tool call for that change, so nothing was changed. What exact target should I use?",
                );
            } else if guidance_requests.is_empty() {
                push_provider_message_if_visible(session, provider_text.to_string());
            }
            return;
        }

        if model_first_provider_text_indicates_uncertainty(provider_text)
            && !explicit_named_desktop_safe_create
        {
            push_controller_message(
                session,
                "I need one clarification before changing files. What exact target and scope should I use?",
            );
            return;
        }

        if should_block_model_first_auto_create_for_capability_question(input, &validated_actions) {
            push_controller_message(
                session,
                "I can create that, but I need an imperative request before changing files. Say `create a folder called X`.",
            );
            return;
        }

        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                session.push_event(Event::Error(ErrorEvent::new(
                    "A proposed action is already waiting. Model-first tool draft was ignored; approve or reject the pending action before creating another.",
                )));
                return;
            }
            PendingActionSelection::Ambiguous => {
                session.push_event(Event::Error(ErrorEvent::new(
                    "Multiple proposed actions are waiting. Model-first tool draft was ignored until this session is repaired.",
                )));
                return;
            }
        }

        let followup_need = VerifiedMemoryNeed::from_input(input);
        let followup_base = explicit_request_base(input, home_dir()).or_else(|| {
            followup_base_path_for_request(session, followup_need.folder, followup_need.plan)
        });
        let mut validated_actions = validated_actions
            .into_iter()
            .map(|validated| {
                retarget_safe_create_to_followup_base(followup_base.as_deref(), validated)
            })
            .collect::<Vec<_>>();

        match self.legacy_controller_model_first_completeness_continuation(
            session,
            input,
            provider_text,
            followup_base.as_deref(),
            &validated_actions,
        ) {
            ModelFirstCompletenessContinuation::NotNeeded => {}
            ModelFirstCompletenessContinuation::ContinueWith(actions) => {
                validated_actions = actions;
            }
            ModelFirstCompletenessContinuation::Blocked => return,
        }

        let mut review_gated_action_proposed = false;
        for validated in validated_actions {
            let action = Action::proposed(
                next_action_id(session),
                validated.request,
                validated.summary,
            );
            let policy_decision = policy_decision_for_model_first_action(mode, &action);
            if policy_decision.kind == PolicyDecisionKind::AllowApply {
                self.apply_policy_approved_legacy_controller_model_first_action(
                    session,
                    action,
                    validated.target_label,
                    policy_decision,
                );
                continue;
            }

            if review_gated_action_proposed {
                session.push_event(Event::Error(ErrorEvent::new(
                    "Additional review-gated model-first tool draft was ignored; approve or reject the pending action before creating another.",
                )));
                continue;
            }

            session.push_event(Event::ActionProposed(
                ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                    .with_target(validated.target_label),
            ));
            let mut record = ActionRecord::new(action);
            record.policy_decision = Some(policy_decision);
            session.push_action(record);
            push_controller_message(session, model_first_proposal_message(mode));
            review_gated_action_proposed = true;
        }
    }

    fn try_apply_legacy_controller_model_first_directory_fallback(
        &self,
        session: &mut Session,
        input: &str,
        _provider_text: &str,
        mode: PermissionPolicyMode,
    ) -> bool {
        if model_first_no_tool_create_directory_fallback_is_blocked(input)
            || route_input(input) != Route::ProposeCreateDirectory
            || model_first_no_tool_directory_fallback_would_truncate_compound_request(input)
        {
            return false;
        }

        let Some(target_path) = legacy_controller_model_first_directory_fallback_target(input)
        else {
            return false;
        };

        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::CreateDirectory(CreateDirectoryAction {
                target_path: target_path.clone(),
            }),
            format!("create directory {}", target_path.display()),
        );
        self.apply_policy_approved_controller_owned_create_action(
            session,
            action,
            target_path.display().to_string(),
            mode,
            "Model-first fallback directory create failed. No verified filesystem result was recorded.",
        )
    }

    fn apply_policy_approved_legacy_controller_model_first_action(
        &self,
        session: &mut Session,
        action: Action,
        target_label: String,
        policy_decision: PolicyDecision,
    ) {
        self.apply_policy_approved_file_action(
            session,
            action,
            target_label,
            policy_decision,
            "Policy-approved create action failed. No verified filesystem result was recorded.",
        );
    }

    fn legacy_controller_model_first_completeness_continuation(
        &self,
        session: &mut Session,
        input: &str,
        provider_text: &str,
        followup_base: Option<&Path>,
        validated_actions: &[ValidatedModelToolAction],
    ) -> ModelFirstCompletenessContinuation {
        let Some(need) =
            model_first_verified_plan_completion_need(session, input, validated_actions)
        else {
            return ModelFirstCompletenessContinuation::NotNeeded;
        };

        let prompt = model_first_completeness_continuation_prompt(input, provider_text, &need);
        match self.request_legacy_controller_model_first_completeness_continuation(
            session,
            &prompt,
            followup_base,
            validated_actions,
            &need,
            elgar_model_tool_definitions(),
        ) {
            ModelFirstCompletenessContinuationAttempt::NoToolCalls
            | ModelFirstCompletenessContinuationAttempt::Incomplete => {}
            ModelFirstCompletenessContinuationAttempt::Done(result) => return result,
        }

        let prompt = model_first_final_completeness_continuation_prompt(input, &need);
        match self.request_legacy_controller_model_first_completeness_continuation(
            session,
            &prompt,
            followup_base,
            validated_actions,
            &need,
            model_first_create_file_tool_definitions(),
        ) {
            ModelFirstCompletenessContinuationAttempt::NoToolCalls
            | ModelFirstCompletenessContinuationAttempt::Incomplete => self
                .legacy_controller_model_first_plan_scaffold_fallback(
                    session,
                    validated_actions,
                    &need,
                ),
            ModelFirstCompletenessContinuationAttempt::Done(result) => result,
        }
    }

    fn legacy_controller_model_first_plan_scaffold_fallback(
        &self,
        session: &mut Session,
        _validated_actions: &[ValidatedModelToolAction],
        need: &ModelFirstPlanCompletenessNeed,
    ) -> ModelFirstCompletenessContinuation {
        let Some(actions) = controller_owned_verified_plan_scaffold_actions(need) else {
            push_model_first_incomplete_plan_message(session, need);
            return ModelFirstCompletenessContinuation::Blocked;
        };

        let project_plan = build_project_scaffold_plan(&need.project_root, &need.plan_contents);
        if let Some(conflict) = first_existing_scaffold_target(&project_plan) {
            push_controller_message(
                session,
                format!(
                    "Controller-owned fallback was not applied because a target already exists: {}.",
                    conflict.display()
                ),
            );
            return ModelFirstCompletenessContinuation::Blocked;
        }

        push_controller_message(
            session,
            "I finished the React TypeScript project from the verified plan.",
        );
        ModelFirstCompletenessContinuation::ContinueWith(actions)
    }

    fn request_legacy_controller_model_first_completeness_continuation(
        &self,
        session: &mut Session,
        prompt: &str,
        followup_base: Option<&Path>,
        validated_actions: &[ValidatedModelToolAction],
        need: &ModelFirstPlanCompletenessNeed,
        tools: Vec<crate::provider::ChatToolDefinition>,
    ) -> ModelFirstCompletenessContinuationAttempt {
        let request = self.provider.request_metadata();
        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        match self
            .provider
            .chat_with_tools_with_metadata(prompt, &request, tools)
        {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                let tool_calls = output.tool_calls.clone();
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider,
                    request.request_id,
                    output,
                )));

                if tool_calls.is_empty() {
                    return ModelFirstCompletenessContinuationAttempt::NoToolCalls;
                }

                let outputs = match validate_model_tool_outputs(&tool_calls) {
                    Ok(outputs) => outputs,
                    Err(error) => {
                        session.push_event(Event::Error(ErrorEvent::new(error.message)));
                        return ModelFirstCompletenessContinuationAttempt::Done(
                            ModelFirstCompletenessContinuation::Blocked,
                        );
                    }
                };

                let mut continuation_actions = Vec::new();
                for output in outputs {
                    match output {
                        ValidatedModelToolOutput::Action(action) => {
                            continuation_actions
                                .push(retarget_safe_create_to_followup_base(followup_base, action));
                        }
                        ValidatedModelToolOutput::Guidance(guidance) => {
                            push_model_first_guidance_message(session, &guidance);
                            return ModelFirstCompletenessContinuationAttempt::Done(
                                ModelFirstCompletenessContinuation::Blocked,
                            );
                        }
                    }
                }

                if !continuation_actions
                    .iter()
                    .all(model_first_action_is_safe_create)
                {
                    push_controller_message(
                        session,
                        "The follow-up tool calls included a review-gated action, so no implementation files were changed.",
                    );
                    return ModelFirstCompletenessContinuationAttempt::Done(
                        ModelFirstCompletenessContinuation::Blocked,
                    );
                }

                let mut combined = validated_actions.to_vec();
                combined.extend(continuation_actions);
                let remaining = missing_expected_verified_plan_files(
                    &need.expected_files,
                    &need.project_root,
                    &combined,
                );
                if !remaining.is_empty() {
                    return ModelFirstCompletenessContinuationAttempt::Incomplete;
                }

                ModelFirstCompletenessContinuationAttempt::Done(
                    ModelFirstCompletenessContinuation::ContinueWith(combined),
                )
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
                ModelFirstCompletenessContinuationAttempt::Done(
                    ModelFirstCompletenessContinuation::Blocked,
                )
            }
        }
    }
}

fn push_model_first_guidance_message(
    session: &mut Session,
    guidance: &ValidatedModelGuidanceRequest,
) {
    push_controller_message(session, guidance.question.trim());
}

fn model_first_no_tool_create_directory_fallback_is_blocked(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    normalized.starts_with("can you ")
        || normalized.starts_with("could you ")
        || normalized.starts_with("would you ")
}

fn legacy_controller_model_first_directory_fallback_target(input: &str) -> Option<PathBuf> {
    match parse_create_directory_target(input)? {
        ParsedCreateDirectoryTarget::ShellDirectory(target_path)
        | ParsedCreateDirectoryTarget::ProjectRelative(target_path) => Some(target_path),
        ParsedCreateDirectoryTarget::ShellDirectories(_) => None,
    }
}
