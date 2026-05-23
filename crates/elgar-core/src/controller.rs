use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    action::{
        Action, ActionRequest, CreateDirectoryAction, DeleteFileAction, MoveFileAction,
        ShellCommandAction, SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS,
    },
    context::{context_budget_tokens, ContextAccounting, ContextBundle},
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage, VerifiedActionResult,
    },
    fs::Filesystem,
    model_runtime::{
        elgar_model_tool_definitions, validate_model_tool_outputs, ValidatedModelGuidanceRequest,
        ValidatedModelToolAction, ValidatedModelToolOutput,
    },
    policy::{PermissionPolicyMode, PolicyDecision, PolicyDecisionKind},
    provider::{
        ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStreamChunk, ProviderStub,
    },
    router::{
        is_prior_project_execution_request, is_project_creation_request,
        normalize_pasted_transcript_input, route_input, strip_action_request_prefixes, Route,
    },
    session::{
        ActionRecord, PendingActionSelection, ProviderMetadata, ProviderPromptMemoryOmittedFact,
        ProviderPromptMemorySelectedFact, ProviderPromptMemorySelection, Session,
        StructuredProjectPlan, StructuredProjectPlanStatus, VerifiedFolderReference,
        VerifiedPlanReference,
    },
    shell::ShellExecutor,
};

/// Controller turn flow over an explicit provider backend.
///
/// The controller records facts into session state. It does not execute actions,
/// mutate files, or treat provider text as truth. The default provider backend
/// is deterministic and no-network; live provider backends require explicit
/// construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controller<P = ProviderStub> {
    pub provider: P,
}

impl<P> Controller<P> {
    pub fn new(provider: P) -> Self {
        Self { provider }
    }

    pub fn refresh_context_accounting(
        &self,
        session: &mut Session,
        max_window_tokens: Option<u64>,
    ) {
        let context_accounting = ContextAccounting::from_default_local_files(
            &session.project_root,
            &session.cwd,
            max_window_tokens,
        );
        session.set_context_accounting(context_accounting);
    }
}

impl Controller<LmStudioProvider> {
    pub fn with_lm_studio_provider(config: ProviderConfig) -> Self {
        Self::new(LmStudioProvider::new(config))
    }
}

impl<P> Controller<P>
where
    P: ControllerProvider,
{
    pub fn turn(&self, session: &mut Session, input: &str) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));

        let normalized_input = normalize_pasted_transcript_input(input);
        let controller_input = normalized_input.as_ref();
        let route = route_input(controller_input);
        match route {
            Route::AskModel => self.handle_ask_model(session, input),
            Route::Help => push_controller_message(session, HELP_MESSAGE),
            Route::Unknown => push_controller_message(session, UNKNOWN_MESSAGE),
            Route::ApproveAction => self.handle_approve_action(session),
            Route::RejectAction => self.handle_reject_action(session),
            Route::ProposeMarkdownPlanFile => {
                self.handle_propose_markdown_plan_file(session, controller_input)
            }
            Route::ProposeWriteFile => self.handle_propose_write_file(session, controller_input),
            Route::ProposePatchFile => self.handle_propose_patch_file(session, controller_input),
            Route::ProposeOverwriteFile => {
                self.handle_propose_overwrite_file(session, controller_input)
            }
            Route::ProposeDeleteFile => self.handle_propose_delete_file(session, controller_input),
            Route::ProposeMoveFile => self.handle_propose_move_file(session, controller_input),
            Route::ProposeCreateDirectory => {
                self.handle_propose_create_directory(session, controller_input)
            }
            Route::ProposeShellCommand => {
                self.handle_propose_shell_command(session, controller_input)
            }
            Route::ExecutePlan => self.handle_execute_plan(session, controller_input),
        }

        TurnResult {
            route,
            events: session.events()[start_index..].to_vec(),
        }
    }

    /// Record an explicit chat turn without asking the router to classify text.
    ///
    /// This is for UI surfaces that already know the input is normal chat.
    /// Permissioned action requests should still use `turn`.
    pub fn model_turn(&self, session: &mut Session, input: &str) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));
        self.handle_ask_model(session, input);

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    /// Record a model-first turn that may draft one or more tool actions.
    ///
    /// The provider can suggest text and tool calls. The controller validates
    /// each draft; policy may auto-apply safe new creates with filesystem
    /// verification in AutoCreateReviewModify, otherwise one action stays
    /// proposed and review-gated.
    pub fn model_first_turn_with_policy(
        &self,
        session: &mut Session,
        input: &str,
        mode: PermissionPolicyMode,
    ) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));
        self.handle_model_first_turn_with_policy(session, input, mode);

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    /// Record an explicit chat turn while exposing provider stream chunks.
    ///
    /// Stream chunks are provider suggestions only. The controller records
    /// durable session facts only after the provider call completes or errors.
    pub fn model_turn_streaming(
        &self,
        session: &mut Session,
        input: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));
        self.handle_ask_model_streaming(session, input, on_chunk);

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    fn handle_ask_model(&self, session: &mut Session, input: &str) {
        self.handle_ask_model_streaming(session, input, &mut |_| {});
    }

    fn handle_ask_model_streaming(
        &self,
        session: &mut Session,
        input: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) {
        let request = self.provider.request_metadata();

        let mut metadata = ProviderMetadata::new(request.provider.clone());
        metadata.model = request.model.clone();
        metadata.request_id = Some(request.request_id.clone());
        session.set_provider_metadata(metadata);

        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        let provider_prompt = provider_prompt_with_context(session, input);
        match self
            .provider
            .chat_stream_with_metadata(&provider_prompt, &request, on_chunk)
        {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider,
                    request.request_id,
                    output.clone(),
                )));
                push_provider_message_if_visible(session, output.text);
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
            }
        }
    }

    fn handle_model_first_turn_with_policy(
        &self,
        session: &mut Session,
        input: &str,
        mode: PermissionPolicyMode,
    ) {
        let normalized_input = normalize_pasted_transcript_input(input);
        let controller_input = normalized_input.as_ref();
        if self.handle_model_first_controller_owned_escape_hatch(session, controller_input, mode) {
            return;
        }

        let request = self.provider.request_metadata();

        let mut metadata = ProviderMetadata::new(request.provider.clone());
        metadata.model = request.model.clone();
        metadata.request_id = Some(request.request_id.clone());
        session.set_provider_metadata(metadata);

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
                push_provider_message_if_visible(session, provider_text.clone());

                match validate_model_tool_outputs(&tool_calls) {
                    Ok(outputs) => {
                        self.handle_validated_model_first_outputs(
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

    fn handle_model_first_controller_owned_escape_hatch(
        &self,
        _session: &mut Session,
        _input: &str,
        _mode: PermissionPolicyMode,
    ) -> bool {
        false
    }

    fn handle_validated_model_first_outputs(
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

        if let Some(guidance) = guidance_requests.first() {
            push_model_first_guidance_message(session, guidance);
            return;
        }

        if validated_actions.is_empty() {
            if should_ask_guidance_for_prose_only_model_first(input, provider_text) {
                push_controller_message(
                    session,
                    "I did not receive a tool call for that change, so nothing was changed. What exact target should I use?",
                );
            }
            return;
        }

        if model_first_provider_text_indicates_uncertainty(provider_text) {
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

        let followup_base = model_first_followup_base_relative_path(session, input);
        let mut review_gated_action_proposed = false;
        for validated in validated_actions {
            let validated = retarget_model_first_safe_create_to_followup_base(
                followup_base.as_deref(),
                validated,
            );
            let action = Action::proposed(
                next_action_id(session),
                validated.request,
                validated.summary,
            );
            let policy_decision = policy_decision_for_model_first_action(mode, &action);
            if policy_decision.kind == PolicyDecisionKind::AllowApply {
                self.apply_policy_approved_model_first_action(
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

    fn apply_policy_approved_model_first_action(
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

    fn apply_controller_owned_markdown_plan_file(
        &self,
        session: &mut Session,
        proposal: ControllerOwnedProjectPlanProposal,
        mode: PermissionPolicyMode,
    ) {
        if !proposal.project_root.exists() {
            let action = Action::proposed(
                next_action_id(session),
                ActionRequest::CreateDirectory(CreateDirectoryAction {
                    target_path: proposal.project_root.clone(),
                }),
                format!("create directory {}", proposal.project_root.display()),
            );
            if !self.apply_policy_approved_controller_owned_create_action(
                session,
                action,
                proposal.project_root.display().to_string(),
                mode,
                "Controller-owned project folder create failed. No verified plan file was written.",
            ) {
                return;
            }
        } else if !proposal.project_root.is_dir() {
            push_controller_message(
                session,
                format!(
                    "Plan target is not a directory: {}.",
                    proposal.project_root.display()
                ),
            );
            return;
        }

        let action = Action::proposed_create_file(
            next_action_id(session),
            proposal.plan_path.clone(),
            proposal.contents,
            format!("create Markdown plan {}", proposal.plan_path.display()),
        );
        self.apply_policy_approved_controller_owned_create_action(
            session,
            action,
            proposal.plan_path.display().to_string(),
            mode,
            "Controller-owned Markdown plan create failed. No verified plan file was recorded.",
        );
    }

    fn apply_policy_approved_controller_owned_create_action(
        &self,
        session: &mut Session,
        action: Action,
        target_label: String,
        mode: PermissionPolicyMode,
        failure_message: &'static str,
    ) -> bool {
        let policy_decision = PolicyDecision::allow_apply(
            mode,
            "safe controller-owned new create action validated by policy",
        );
        self.apply_policy_approved_file_action(
            session,
            action,
            target_label,
            policy_decision,
            failure_message,
        )
    }

    fn apply_policy_approved_file_action(
        &self,
        session: &mut Session,
        action: Action,
        target_label: String,
        policy_decision: PolicyDecision,
        failure_message: &'static str,
    ) -> bool {
        let approved = action.approve();
        let allowed_root = policy_allowed_root_for_action(session, &approved);
        let index = session.actions().len();
        let mut record = ActionRecord::new(approved.clone());
        record.policy_decision = Some(policy_decision);
        session.push_action(record);
        session.push_event(Event::ActionApproved(
            ActionEvent::new(
                approved.id.clone(),
                approved.kind(),
                approved.summary.clone(),
            )
            .with_target(target_label),
        ));

        apply_approved_file_action_at_index(
            session,
            index,
            &approved,
            &allowed_root,
            failure_message,
        )
    }

    fn handle_propose_markdown_plan_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another Markdown plan file.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        match controller_owned_project_plan_proposal(input, session) {
            Ok(Some(proposal)) => {
                self.propose_shell_write_markdown_plan_file_with_expected_directories(
                    session,
                    proposal.plan_path,
                    proposal.contents,
                    vec![proposal.project_root],
                );
                return;
            }
            Ok(None) => {}
            Err(message) => {
                push_controller_message(session, message);
                return;
            }
        }

        let target_path = match parse_markdown_plan_target(input, session) {
            Ok(target_path) => target_path,
            Err(message) => {
                push_controller_message(session, message);
                return;
            }
        };
        let request = self.provider.request_metadata();

        let mut metadata = ProviderMetadata::new(request.provider.clone());
        metadata.model = request.model.clone();
        metadata.request_id = Some(request.request_id.clone());
        session.set_provider_metadata(metadata);

        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        let prompt = markdown_plan_prompt(input, &target_path);
        let provider_prompt = provider_prompt_with_context(session, &prompt);
        match self.provider.chat_with_metadata(&provider_prompt, &request) {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                let contents = normalize_markdown_plan_contents(&output.text);
                if contents.trim().is_empty() {
                    session.push_event(Event::Error(ErrorEvent::new(format!(
                        "{} provider request {} returned an empty Markdown plan",
                        request.provider, request.request_id
                    ))));
                    return;
                }

                let provider_text = output.text.clone();
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider,
                    request.request_id,
                    output,
                )));
                push_provider_message_if_visible(session, provider_text);

                if target_path.is_absolute() {
                    self.propose_shell_write_markdown_plan_file(session, target_path, contents);
                } else {
                    let action = Action::proposed_create_file(
                        next_action_id(session),
                        target_path.clone(),
                        contents,
                        format!("create Markdown plan {}", target_path.display()),
                    );
                    session.push_event(Event::ActionProposed(
                        ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                            .with_target(target_path.display().to_string()),
                    ));
                    session.push_action(ActionRecord::new(action));
                    push_controller_message(
                        session,
                        "Proposed Markdown CreateFile action. Approve or reject before any file is written.",
                    );
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

    fn propose_shell_write_markdown_plan_file(
        &self,
        session: &mut Session,
        target_path: PathBuf,
        contents: String,
    ) {
        self.propose_shell_write_markdown_plan_file_with_expected_directories(
            session,
            target_path,
            contents,
            Vec::new(),
        );
    }

    fn propose_shell_write_markdown_plan_file_with_expected_directories(
        &self,
        session: &mut Session,
        target_path: PathBuf,
        contents: String,
        expected_directories: Vec<PathBuf>,
    ) {
        let command = shell_write_file_command(&target_path, &contents);
        let mut shell_command = ShellCommandAction::new(command.clone(), session.cwd.clone());
        shell_command.timeout_seconds = SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS;
        shell_command.expected_effect = format!(
            "Write Markdown plan {} and verify it exists.",
            target_path.display()
        );
        shell_command.expected_file = Some(target_path.clone());
        let expected_directories = dedupe_paths(expected_directories);
        if expected_directories.len() == 1 {
            shell_command.expected_directory = expected_directories.first().cloned();
        } else if !expected_directories.is_empty() {
            shell_command.expected_directories = expected_directories;
        }
        shell_command.risk_notes =
            "Writes a local Markdown file through an approved shell command; filesystem confirmation is required after execution."
                .to_string();

        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::ShellCommand(shell_command),
            format!("create Markdown plan {}", target_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(command),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed Markdown ShellCommand action. Approve or reject before any file is written.",
        );
    }

    fn handle_propose_write_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another CreateFile action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some(target_path) = parse_write_file_target(input) else {
            push_controller_message(
                session,
                "CreateFile request was recognized, but no target path could be parsed.",
            );
            return;
        };

        let action = Action::proposed_write_file(
            next_action_id(session),
            target_path.clone(),
            "",
            format!("write {}", target_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(target_path.display().to_string()),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed CreateFile action. Approve or reject before any file is written.",
        );
    }

    fn handle_propose_patch_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another PatchFile action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some((target_path, find, replace)) = parse_patch_file_request(input) else {
            push_controller_message(
                session,
                "PatchFile request was recognized, but target/find/replace data could not be parsed.",
            );
            return;
        };

        let action = Action::proposed_patch_file(
            next_action_id(session),
            target_path.clone(),
            find,
            replace,
            format!("edit {}", target_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(target_path.display().to_string()),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed PatchFile action. Approve or reject before any file is changed.",
        );
    }

    fn handle_propose_overwrite_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another OverwriteFile action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some((target_path, contents)) = parse_overwrite_file_request(input) else {
            push_controller_message(
                session,
                "OverwriteFile request was recognized, but target/content data could not be parsed.",
            );
            return;
        };

        let action = Action::proposed_overwrite_file(
            next_action_id(session),
            target_path.clone(),
            contents,
            format!("overwrite {}", target_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(target_path.display().to_string()),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed OverwriteFile action. Approve or reject before any file is changed.",
        );
    }

    fn handle_propose_delete_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another DeleteFile action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some(target_path) = parse_delete_file_target(input) else {
            push_controller_message(
                session,
                "DeleteFile request was recognized, but no target path could be parsed.",
            );
            return;
        };

        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::DeleteFile(DeleteFileAction {
                target_path: target_path.clone(),
            }),
            format!("delete {}", target_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(target_path.display().to_string()),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed DeleteFile action. Approve or reject before any file is deleted.",
        );
    }

    fn handle_propose_move_file(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another MoveFile action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some((source_path, target_path)) = parse_move_file_request(input) else {
            push_controller_message(
                session,
                "MoveFile request was recognized, but source/target paths could not be parsed.",
            );
            return;
        };

        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::MoveFile(MoveFileAction {
                source_path: source_path.clone(),
                target_path: target_path.clone(),
            }),
            format!(
                "move {} to {}",
                source_path.display(),
                target_path.display()
            ),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(action_target_label(&action)),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed MoveFile action. Approve or reject before any file is moved.",
        );
    }

    fn handle_propose_create_directory(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another CreateDirectory action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let target = match parse_create_directory_target(input) {
            Some(target) => Some(target),
            None => match parse_create_directory_plan_followup_target(session, input) {
                Ok(target) => target,
                Err(message) => {
                    push_controller_message(session, message);
                    return;
                }
            },
        };

        let Some(target) = target else {
            push_controller_message(
                session,
                "CreateDirectory request was recognized, but no target path could be parsed.",
            );
            return;
        };
        match target {
            ParsedCreateDirectoryTarget::ProjectRelative(target_path) => {
                let action = Action::proposed(
                    next_action_id(session),
                    ActionRequest::CreateDirectory(CreateDirectoryAction {
                        target_path: target_path.clone(),
                    }),
                    format!("create directory {}", target_path.display()),
                );
                session.push_event(Event::ActionProposed(
                    ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                        .with_target(target_path.display().to_string()),
                ));
                session.push_action(ActionRecord::new(action));
                push_controller_message(session, create_directory_proposal_message(&[target_path]));
            }
            ParsedCreateDirectoryTarget::ShellDirectory(target_path) => {
                self.propose_shell_create_directory(session, target_path);
            }
            ParsedCreateDirectoryTarget::ShellDirectories(target_paths) => {
                self.propose_shell_create_directories(session, target_paths);
            }
        }
    }

    fn propose_shell_create_directory(&self, session: &mut Session, target_path: PathBuf) {
        self.propose_shell_create_directories(session, vec![target_path]);
    }

    fn propose_shell_create_directories(&self, session: &mut Session, target_paths: Vec<PathBuf>) {
        let target_paths = dedupe_paths(target_paths);
        let Some(first_target) = target_paths.first().cloned() else {
            push_controller_message(
                session,
                "CreateDirectory request was recognized, but no target path could be parsed.",
            );
            return;
        };
        let command = format!(
            "mkdir -p {}",
            target_paths
                .iter()
                .map(|target_path| shell_quote_path(target_path))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut shell_command = ShellCommandAction::new(command.clone(), session.cwd.clone());
        shell_command.timeout_seconds = SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS;
        if target_paths.len() == 1 {
            shell_command.expected_effect = format!(
                "Create directory {} and verify it exists.",
                first_target.display()
            );
            shell_command.expected_directory = Some(first_target.clone());
        } else {
            shell_command.expected_effect = format!(
                "Create directories {} and verify they exist.",
                display_path_list(&target_paths)
            );
            shell_command.expected_directories = target_paths.clone();
        }
        shell_command.risk_notes =
            "Creates a local directory through the approved shell command; filesystem confirmation is required after execution."
                .to_string();

        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::ShellCommand(shell_command),
            if target_paths.len() == 1 {
                format!("create directory {}", first_target.display())
            } else {
                format!("create directories {}", display_path_list(&target_paths))
            },
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(command),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(session, create_directory_proposal_message(&target_paths));
    }

    fn handle_propose_shell_command(&self, session: &mut Session, input: &str) {
        match session.pending_action_selection() {
            PendingActionSelection::None => {}
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Approve or reject it before requesting another ShellCommand action.",
                );
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        }

        let Some(command) = parse_shell_command_request(input) else {
            push_controller_message(
                session,
                "ShellCommand request was recognized, but no command could be parsed.",
            );
            return;
        };

        let mut shell_command = ShellCommandAction::new(command.clone(), session.cwd.clone());
        shell_command.timeout_seconds = SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS;
        let action = Action::proposed(
            next_action_id(session),
            ActionRequest::ShellCommand(shell_command),
            format!("run shell command {command}"),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(command),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed ShellCommand action. Approve or reject before any command is run.",
        );
    }

    fn handle_execute_plan(&self, session: &mut Session, input: &str) {
        self.handle_execute_plan_with_policy(session, input, None);
    }

    fn handle_execute_plan_with_policy(
        &self,
        session: &mut Session,
        input: &str,
        mode: Option<PermissionPolicyMode>,
    ) {
        match session.pending_action_selection() {
            PendingActionSelection::Single(_) => {
                push_controller_message(
                    session,
                    "A proposed action is already waiting. Use /approve or /reject.",
                );
            }
            PendingActionSelection::None => {
                let Some(plan_path) = latest_verified_markdown_plan_file(session) else {
                    if is_prior_project_execution_request(input) {
                        match missing_project_plan_proposal_for_latest_folder(input, session) {
                            Ok(proposal) => {
                                if mode == Some(PermissionPolicyMode::AutoCreateReviewModify) {
                                    self.apply_controller_owned_markdown_plan_file(
                                        session,
                                        proposal,
                                        PermissionPolicyMode::AutoCreateReviewModify,
                                    );
                                } else {
                                    self.propose_shell_write_markdown_plan_file_with_expected_directories(
                                        session,
                                        proposal.plan_path,
                                        proposal.contents,
                                        vec![proposal.project_root],
                                    );
                                }
                            }
                            Err(message) => push_controller_message(session, message),
                        }
                        return;
                    }
                    push_controller_message(
                        session,
                        "No controller-owned executable plan is waiting. Ask for a specific file, folder, shell command, or Markdown plan first.",
                    );
                    return;
                };
                let Ok(plan_contents) = std::fs::read_to_string(&plan_path) else {
                    push_controller_message(
                        session,
                        "The latest verified Markdown plan could not be read. Recreate the plan before executing it.",
                    );
                    return;
                };
                let base_path = if references_prior_folder(input) {
                    match latest_existing_verified_directory_reference(session) {
                        Ok(path) => path,
                        Err(message) => {
                            push_controller_message(session, message);
                            return;
                        }
                    }
                } else {
                    let Some(base_path) = plan_path
                        .parent()
                        .map(Path::to_path_buf)
                        .or_else(|| latest_verified_directory_reference(session))
                    else {
                        push_controller_message(
                            session,
                            "The latest verified Markdown plan has no executable target folder.",
                        );
                        return;
                    };
                    base_path
                };

                if mode == Some(PermissionPolicyMode::AutoCreateReviewModify) {
                    self.apply_controller_owned_markdown_plan_scaffold(
                        session,
                        plan_path,
                        base_path,
                        &plan_contents,
                        PermissionPolicyMode::AutoCreateReviewModify,
                    );
                } else {
                    self.propose_shell_execute_markdown_plan(
                        session,
                        plan_path,
                        base_path,
                        &plan_contents,
                    );
                }
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
            }
        }
    }

    fn propose_shell_execute_markdown_plan(
        &self,
        session: &mut Session,
        source_plan_path: PathBuf,
        base_path: PathBuf,
        plan_contents: &str,
    ) {
        let project_plan = build_project_scaffold_plan(&base_path, plan_contents);
        let command =
            shell_write_many_files_command(&project_plan.directories, &project_plan.files);
        let expected_files = project_plan
            .files
            .iter()
            .map(|(path, _contents)| path.clone())
            .collect::<Vec<_>>();
        let action_id = next_action_id(session);
        session.record_structured_project_plan(StructuredProjectPlan {
            source_action_id: Some(action_id.clone()),
            source_plan_path,
            project_root: base_path.clone(),
            stage: "scaffold".to_string(),
            status: StructuredProjectPlanStatus::Proposed,
            expected_directories: project_plan.directories.clone(),
            expected_files: expected_files.clone(),
        });

        let mut shell_command = ShellCommandAction::new(command.clone(), session.cwd.clone());
        shell_command.timeout_seconds = SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS;
        shell_command.expected_effect = format!(
            "Create project files under {} and verify them.",
            base_path.display()
        );
        shell_command.expected_directories = project_plan.directories.clone();
        shell_command.expected_files = expected_files;
        shell_command.risk_notes =
            "Creates local project directories and files through an approved shell command; filesystem confirmation is required after execution."
                .to_string();

        let action = Action::proposed(
            action_id,
            ActionRequest::ShellCommand(shell_command),
            format!("execute Markdown plan in {}", base_path.display()),
        );
        session.push_event(Event::ActionProposed(
            ActionEvent::new(action.id.clone(), action.kind(), action.summary.clone())
                .with_target(command),
        ));
        session.push_action(ActionRecord::new(action));
        push_controller_message(
            session,
            "Proposed ShellCommand action to execute the verified Markdown plan. Approve or reject before any files are written.",
        );
    }

    fn apply_controller_owned_markdown_plan_scaffold(
        &self,
        session: &mut Session,
        source_plan_path: PathBuf,
        base_path: PathBuf,
        plan_contents: &str,
        mode: PermissionPolicyMode,
    ) {
        let project_plan = build_project_scaffold_plan(&base_path, plan_contents);
        let expected_files = project_plan
            .files
            .iter()
            .map(|(path, _contents)| path.clone())
            .collect::<Vec<_>>();

        if project_plan.files.is_empty() {
            let action = Action::proposed_create_file(
                next_action_id(session),
                base_path.join(".elgar-unsupported-plan"),
                "",
                format!("execute Markdown plan in {}", base_path.display()),
            );
            record_policy_approved_action_failure(
                session,
                action,
                base_path.display().to_string(),
                mode,
                "verified Markdown plan did not describe any supported project files".to_string(),
                "Controller-owned scaffold was not applied because the verified plan does not describe supported project files.",
            );
            return;
        }

        if let Some(conflict) = first_existing_scaffold_target(&project_plan) {
            let action = Action::proposed_create_file(
                next_action_id(session),
                conflict.clone(),
                "",
                format!("execute Markdown plan in {}", base_path.display()),
            );
            record_policy_approved_action_failure(
                session,
                action,
                conflict.display().to_string(),
                mode,
                format!("scaffold target already exists: {}", conflict.display()),
                "Controller-owned scaffold was not applied because a target already exists.",
            );
            return;
        }

        let first_action_id = format!("action-{}", session.actions().len() + 1);
        let mut applied_all = true;
        for directory in &project_plan.directories {
            let action = Action::proposed(
                next_action_id(session),
                ActionRequest::CreateDirectory(CreateDirectoryAction {
                    target_path: directory.clone(),
                }),
                format!("create directory {}", directory.display()),
            );
            applied_all = self.apply_policy_approved_controller_owned_create_action(
                session,
                action,
                directory.display().to_string(),
                mode,
                "Controller-owned scaffold directory create failed. No verified scaffold result was recorded.",
            ) && applied_all;
            if !applied_all {
                return;
            }
        }

        for (path, contents) in &project_plan.files {
            let action = Action::proposed_create_file(
                next_action_id(session),
                path.clone(),
                contents.clone(),
                format!("create project file {}", path.display()),
            );
            applied_all = self.apply_policy_approved_controller_owned_create_action(
                session,
                action,
                path.display().to_string(),
                mode,
                "Controller-owned scaffold file create failed. No verified scaffold result was recorded.",
            ) && applied_all;
            if !applied_all {
                return;
            }
        }

        session.record_verified_plan_reference(VerifiedPlanReference {
            path: source_plan_path.clone(),
            project_root: base_path.clone(),
            source_action_id: first_action_id.clone(),
        });
        session.record_structured_project_plan(StructuredProjectPlan {
            source_action_id: Some(first_action_id),
            source_plan_path,
            project_root: base_path,
            stage: "scaffold".to_string(),
            status: StructuredProjectPlanStatus::Executed,
            expected_directories: project_plan.directories,
            expected_files,
        });
    }

    fn handle_reject_action(&self, session: &mut Session) {
        let index = match session.pending_action_selection() {
            PendingActionSelection::Single(index) => index,
            PendingActionSelection::None => {
                push_controller_message(session, "No proposed action is waiting for rejection.");
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        };

        let rejected = session.actions()[index].action.reject();
        session.remove_structured_project_plan_for_action(&rejected.id);
        let record = session
            .action_mut(index)
            .expect("latest proposed action index must reference an action record");
        record.action = rejected.clone();
        session.push_event(Event::ActionRejected(
            ActionEvent::new(
                rejected.id.clone(),
                rejected.kind(),
                rejected.summary.clone(),
            )
            .with_target(action_target_label(&rejected)),
        ));
        push_controller_message(session, "Rejected action. No filesystem change was made.");
    }

    fn handle_approve_action(&self, session: &mut Session) {
        let index = match session.pending_action_selection() {
            PendingActionSelection::Single(index) => index,
            PendingActionSelection::None => {
                push_controller_message(session, "No proposed action is waiting for approval.");
                return;
            }
            PendingActionSelection::Ambiguous => {
                push_ambiguous_pending_action_message(session);
                return;
            }
        };

        let approved = session.actions()[index].action.approve();
        let record = session
            .action_mut(index)
            .expect("latest proposed action index must reference an action record");
        record.action = approved.clone();
        session.push_event(Event::ActionApproved(
            ActionEvent::new(
                approved.id.clone(),
                approved.kind(),
                approved.summary.clone(),
            )
            .with_target(action_target_label(&approved)),
        ));

        if let ActionRequest::ShellCommand(shell_command) = &approved.request {
            match ShellExecutor::execute(shell_command) {
                Ok(result) => match verify_expected_shell_effect(shell_command, result) {
                    Ok(result) => {
                        let message = verified_action_success_message(session, &approved, &result);
                        let record = session
                            .action_mut(index)
                            .expect("approved action index must reference an action record");
                        record.verified_result = Some(result.clone());
                        record.failure_reason = None;
                        record.action = approved.mark_applied();
                        record_verified_project_memory(session, &approved, &result);
                        session.mark_structured_project_plan_executed(&approved.id);
                        session.push_event(Event::ActionApplied(ActionApplied::new(
                            approved.id.clone(),
                            approved.kind(),
                            result,
                        )));
                        push_controller_message(session, message);
                    }
                    Err(reason) => {
                        let record = session
                            .action_mut(index)
                            .expect("approved action index must reference an action record");
                        record.verified_result = None;
                        record.failure_reason = Some(reason.clone());
                        record.action = approved.mark_failed();
                        session.remove_structured_project_plan_for_action(&approved.id);
                        session.push_event(Event::ActionFailed(ActionFailed::new(
                            approved.id.clone(),
                            approved.kind(),
                            reason,
                        )));
                        push_controller_message(
                            session,
                            "Approved shell command ran, but expected filesystem verification failed.",
                        );
                    }
                },
                Err(error) => {
                    let reason = error.to_string();
                    let record = session
                        .action_mut(index)
                        .expect("approved action index must reference an action record");
                    record.verified_result = None;
                    record.failure_reason = Some(reason.clone());
                    record.action = approved.mark_failed();
                    session.remove_structured_project_plan_for_action(&approved.id);
                    session.push_event(Event::ActionFailed(ActionFailed::new(
                        approved.id.clone(),
                        approved.kind(),
                        reason,
                    )));
                    push_controller_message(
                        session,
                        "Approved shell command failed before a shell result could be recorded.",
                    );
                }
            }
            return;
        }

        apply_approved_file_action_at_index(
            session,
            index,
            &approved,
            &session.project_root.clone(),
            "Approved file action failed. No verified filesystem result was recorded.",
        );
    }
}

impl Default for Controller<ProviderStub> {
    fn default() -> Self {
        Self::new(ProviderStub::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnResult {
    pub route: Route,
    pub events: Vec<Event>,
}

const HELP_MESSAGE: &str =
    "Elgar core harness can classify help, model questions, file-action requests, shell-command requests, approvals, and rejections.";
const UNKNOWN_MESSAGE: &str =
    "Input was not recognized. No provider, file, action, or shell operation was run.";
const AMBIGUOUS_PENDING_ACTION_MESSAGE: &str =
    "Multiple proposed actions are waiting. Elgar will not approve, reject, or create another action until this session is repaired.";

fn push_controller_message(session: &mut Session, message: impl Into<String>) {
    let message =
        truth_guard_visible_message(session, message.into(), AssistantMessageSource::Controller);
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
}

fn push_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message =
        truth_guard_visible_message(session, message.into(), AssistantMessageSource::Provider);
    if message.trim().is_empty() {
        return;
    }

    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Provider,
    )));
}

fn truth_guard_visible_message(
    session: &Session,
    message: String,
    _source: AssistantMessageSource,
) -> String {
    let normalized = message.to_ascii_lowercase();
    if denies_verified_folder_create(&normalized) {
        if let Some(path) = latest_verified_created_directory(session) {
            return format!(
                "Filesystem truth: {} was created and verified.",
                user_display_path(&path)
            );
        }
    }

    if denies_verified_file_create(&normalized) {
        if let Some(path) = latest_verified_created_file(session) {
            return format!(
                "Filesystem truth: {} was created and verified.",
                user_display_path(&path)
            );
        }
    }

    message
}

fn denies_verified_folder_create(normalized: &str) -> bool {
    (normalized.contains("no folder") || normalized.contains("no directory"))
        && (normalized.contains("was created")
            || normalized.contains("were created")
            || normalized.contains("has been created"))
}

fn denies_verified_file_create(normalized: &str) -> bool {
    normalized.contains("no file")
        && (normalized.contains("was created")
            || normalized.contains("were created")
            || normalized.contains("has been created"))
}

fn latest_verified_created_directory(session: &Session) -> Option<PathBuf> {
    session.actions().iter().rev().find_map(|record| {
        let verified = record.verified_result.as_ref()?;
        match verified {
            VerifiedActionResult::File(
                crate::action::FileActionVerification::DirectoryCreated { path },
            ) => Some(PathBuf::from(path)),
            VerifiedActionResult::Shell(shell) => shell
                .verified_effect
                .as_deref()
                .and_then(|effect| {
                    verified_effect_value(effect, "verified directory exists: ")
                        .or_else(|| verified_effect_value(effect, "verified directories exist: "))
                })
                .and_then(first_verified_effect_path),
            _ => None,
        }
    })
}

fn latest_verified_created_file(session: &Session) -> Option<PathBuf> {
    session.actions().iter().rev().find_map(|record| {
        let verified = record.verified_result.as_ref()?;
        match verified {
            VerifiedActionResult::FileWritten { path }
            | VerifiedActionResult::File(crate::action::FileActionVerification::FileCreated {
                path,
            }) => Some(PathBuf::from(path)),
            VerifiedActionResult::Shell(shell) => shell
                .verified_effect
                .as_deref()
                .and_then(|effect| {
                    verified_effect_value(effect, "verified file exists: ")
                        .or_else(|| verified_effect_value(effect, "verified files exist: "))
                })
                .and_then(first_verified_effect_path),
            _ => None,
        }
    })
}

fn first_verified_effect_path(value: &str) -> Option<PathBuf> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn verified_effect_value<'a>(effect: &'a str, prefix: &str) -> Option<&'a str> {
    effect
        .split("; ")
        .find_map(|part| part.strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn create_directory_proposal_message(target_paths: &[PathBuf]) -> String {
    if target_paths.len() == 1 {
        return format!(
            "I can create {}. Approve to create it.",
            user_display_path(&target_paths[0])
        );
    }

    format!(
        "I can create these directories: {}. Approve to create them.",
        display_user_path_list(target_paths)
    )
}

fn model_first_proposal_message(mode: PermissionPolicyMode) -> String {
    format!(
        "Model-first tool call validated under {mode:?}. Proposed action only. Approve or reject before anything changes."
    )
}

fn policy_decision_for_model_first_action(
    mode: PermissionPolicyMode,
    action: &Action,
) -> PolicyDecision {
    match (mode, &action.request) {
        (
            PermissionPolicyMode::AutoCreateReviewModify,
            ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_),
        ) => PolicyDecision::allow_apply(
            mode,
            "safe new create action validated by model-first tool call",
        ),
        (PermissionPolicyMode::AutoCreateReviewModify, _) => PolicyDecision::require_review(
            mode,
            "modify, delete, move, and shell actions require review",
        ),
        _ => PolicyDecision::require_review(mode, "policy mode requires user review"),
    }
}

fn push_model_first_guidance_message(
    session: &mut Session,
    guidance: &ValidatedModelGuidanceRequest,
) {
    push_controller_message(session, guidance.question.trim());
}

fn should_ask_guidance_for_prose_only_model_first(input: &str, provider_text: &str) -> bool {
    is_model_first_execution_like_request(input)
        || model_first_provider_text_claims_execution(provider_text)
}

fn is_model_first_execution_like_request(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    contains_any_word(
        normalized,
        &[
            "create",
            "implement",
            "make",
            "build",
            "scaffold",
            "write",
            "add",
            "edit",
            "delete",
            "move",
            "rename",
            "run",
        ],
    )
}

fn model_first_provider_text_claims_execution(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    contains_any(
        &normalized,
        &[
            "i created",
            "created ",
            "i wrote",
            "wrote ",
            "i edited",
            "edited ",
            "i updated",
            "updated ",
            "i implemented",
            "implemented ",
            "i ran",
            "ran ",
            "done,",
        ],
    )
}

fn model_first_provider_text_indicates_uncertainty(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    contains_any(
        &normalized,
        &[
            "i'm not sure",
            "i am not sure",
            "i don't know",
            "i do not know",
            "not sure which",
            "not sure what",
            "need clarification",
            "need guidance",
            "unclear",
            "ambiguous",
            "which folder",
            "which file",
            "which target",
        ],
    )
}

fn contains_any_word(value: &str, words: &[&str]) -> bool {
    value
        .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
        .any(|token| words.contains(&token))
}

fn should_block_model_first_auto_create_for_capability_question(
    input: &str,
    validated_actions: &[ValidatedModelToolAction],
) -> bool {
    is_capability_question_prompt(input)
        && validated_actions
            .iter()
            .any(|action| is_safe_create_request(&action.request))
}

fn is_safe_create_request(request: &ActionRequest) -> bool {
    matches!(
        request,
        ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_)
    )
}

fn is_capability_question_prompt(input: &str) -> bool {
    let normalized = input.trim_start().to_ascii_lowercase();
    let normalized = normalized
        .strip_prefix('>')
        .unwrap_or(&normalized)
        .trim_start();
    normalized.starts_with("can you ")
        || normalized.starts_with("could you ")
        || normalized.starts_with("would you ")
}

fn policy_allowed_root_for_action(session: &Session, action: &Action) -> PathBuf {
    let Some(target_path) = action_filesystem_target(action) else {
        return session.project_root.clone();
    };

    if !target_path.is_absolute() {
        return session.project_root.clone();
    }

    if let Some(desktop) = home_dir().map(|home| home.join("Desktop")) {
        if target_path.starts_with(&desktop) {
            return desktop;
        }
    }

    if target_path.starts_with(&session.project_root) {
        return session.project_root.clone();
    }

    session.project_root.clone()
}

fn action_filesystem_target(action: &Action) -> Option<&Path> {
    match &action.request {
        ActionRequest::CreateFile(create_file) => Some(&create_file.target_path),
        ActionRequest::CreateDirectory(create_directory) => Some(&create_directory.target_path),
        ActionRequest::PatchFile(patch_file) => Some(&patch_file.target_path),
        ActionRequest::OverwriteFile(overwrite_file) => Some(&overwrite_file.target_path),
        ActionRequest::DeleteFile(delete_file) => Some(&delete_file.target_path),
        ActionRequest::MoveFile(move_file) => Some(&move_file.target_path),
        ActionRequest::ShellCommand(_) => None,
    }
}

fn first_existing_scaffold_target(project_plan: &ProjectScaffoldPlan) -> Option<PathBuf> {
    project_plan
        .directories
        .iter()
        .chain(project_plan.files.iter().map(|(path, _contents)| path))
        .find(|path| path.try_exists().unwrap_or(true))
        .cloned()
}

fn retarget_model_first_safe_create_to_followup_base(
    base: Option<&Path>,
    mut validated: crate::model_runtime::ValidatedModelToolAction,
) -> crate::model_runtime::ValidatedModelToolAction {
    let Some(base) = base else {
        return validated;
    };

    match &mut validated.request {
        ActionRequest::CreateFile(create_file) => {
            if should_retarget_model_first_create(&create_file.target_path, &base) {
                create_file.target_path = base.join(&create_file.target_path);
            }
        }
        ActionRequest::CreateDirectory(create_directory) => {
            if should_retarget_model_first_create(&create_directory.target_path, &base) {
                create_directory.target_path = base.join(&create_directory.target_path);
            }
        }
        _ => return validated,
    }

    validated.target_label = validated.request.approval_target();
    validated
}

fn model_first_followup_base_relative_path(session: &Session, input: &str) -> Option<PathBuf> {
    let need = VerifiedMemoryNeed::from_input(input);
    if !need.any() {
        return None;
    }

    let memory = session.project_memory();
    if need.plan {
        if let Some(path) = memory
            .latest_structured_plan()
            .map(|plan| &plan.project_root)
            .filter(|path| path.is_dir())
            .and_then(|path| relative_project_path(session, path))
        {
            return Some(path);
        }
        if let Some(path) = memory
            .latest_verified_plan()
            .map(|plan| &plan.project_root)
            .filter(|path| path.is_dir())
            .and_then(|path| relative_project_path(session, path))
        {
            return Some(path);
        }
    }

    if need.folder {
        return memory
            .latest_verified_folder()
            .map(|reference| &reference.path)
            .filter(|path| path.is_dir())
            .and_then(|path| relative_project_path(session, path));
    }

    None
}

fn relative_project_path(session: &Session, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(&session.project_root).ok()?;
    if relative.as_os_str().is_empty() {
        None
    } else {
        Some(relative.to_path_buf())
    }
}

fn should_retarget_model_first_create(target_path: &Path, base: &Path) -> bool {
    !target_path.is_absolute() && !target_path.starts_with(base)
}

fn apply_approved_file_action_at_index(
    session: &mut Session,
    index: usize,
    approved: &Action,
    allowed_root: &Path,
    failure_message: &'static str,
) -> bool {
    match Filesystem::apply_file_action(approved, allowed_root) {
        Ok(result) => {
            let message = verified_action_success_message(session, approved, &result);
            let record = session
                .action_mut(index)
                .expect("approved action index must reference an action record");
            record.verified_result = Some(result.clone());
            record.failure_reason = None;
            record.action = approved.mark_applied();
            record_verified_project_memory(session, approved, &result);
            session.push_event(Event::ActionApplied(ActionApplied::new(
                approved.id.clone(),
                approved.kind(),
                result,
            )));
            push_controller_message(session, message);
            true
        }
        Err(error) => {
            let reason = error.to_string();
            let record = session
                .action_mut(index)
                .expect("approved action index must reference an action record");
            record.verified_result = None;
            record.failure_reason = Some(reason.clone());
            record.action = approved.mark_failed();
            session.push_event(Event::ActionFailed(ActionFailed::new(
                approved.id.clone(),
                approved.kind(),
                reason,
            )));
            push_controller_message(session, failure_message);
            false
        }
    }
}

fn record_policy_approved_action_failure(
    session: &mut Session,
    action: Action,
    target_label: String,
    mode: PermissionPolicyMode,
    reason: String,
    failure_message: &'static str,
) {
    let approved = action.approve();
    let mut record = ActionRecord::new(approved.mark_failed());
    record.policy_decision = Some(PolicyDecision::allow_apply(
        mode,
        "safe controller-owned new create action validated by policy",
    ));
    record.failure_reason = Some(reason.clone());
    session.push_action(record);
    session.push_event(Event::ActionApproved(
        ActionEvent::new(
            approved.id.clone(),
            approved.kind(),
            approved.summary.clone(),
        )
        .with_target(target_label),
    ));
    session.push_event(Event::ActionFailed(ActionFailed::new(
        approved.id.clone(),
        approved.kind(),
        reason,
    )));
    push_controller_message(session, failure_message);
}

fn verified_action_success_message(
    session: &Session,
    action: &Action,
    result: &VerifiedActionResult,
) -> String {
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            let path = resolve_project_path(&session.project_root, &create_directory.target_path);
            format!("Created {}.", user_display_path(&path))
        }
        ActionRequest::ShellCommand(shell_command) => {
            let directories = verified_shell_expected_directories(shell_command);
            if directories.len() == 1 {
                return format!("Created {}.", user_display_path(&directories[0]));
            }
            if !directories.is_empty() {
                return format!("Created {}.", display_user_path_list(&directories));
            }
            "Executed approved shell command and recorded the verified result.".to_string()
        }
        _ => verified_file_action_success_message(result),
    }
}

fn verified_file_action_success_message(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("Wrote {}.", user_display_path(Path::new(path)))
        }
        VerifiedActionResult::File(file) => match file {
            crate::action::FileActionVerification::FileCreated { path } => {
                format!("Created {}.", user_display_path(Path::new(path)))
            }
            crate::action::FileActionVerification::FilePatched { path } => {
                format!("Updated {}.", user_display_path(Path::new(path)))
            }
            crate::action::FileActionVerification::FileOverwritten { path } => {
                format!("Overwrote {}.", user_display_path(Path::new(path)))
            }
            crate::action::FileActionVerification::FileDeleted { path } => {
                format!("Deleted {}.", user_display_path(Path::new(path)))
            }
            crate::action::FileActionVerification::FileMoved {
                source_path,
                target_path,
            } => format!(
                "Moved {} to {}.",
                user_display_path(Path::new(source_path)),
                user_display_path(Path::new(target_path))
            ),
            crate::action::FileActionVerification::DirectoryCreated { path } => {
                format!("Created {}.", user_display_path(Path::new(path)))
            }
        },
        VerifiedActionResult::Shell(_) => {
            "Applied approved action and recorded the verified result.".to_string()
        }
    }
}

fn display_user_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| user_display_path(path))
        .collect::<Vec<_>>()
        .join(", ")
}

fn user_display_path(path: &Path) -> String {
    if let Some(home) = home_dir() {
        let desktop = home.join("Desktop");
        if path == desktop {
            return "Desktop".to_string();
        }
        if let Ok(relative) = path.strip_prefix(&desktop) {
            return PathBuf::from("Desktop")
                .join(relative)
                .display()
                .to_string();
        }
    }

    path.display().to_string()
}

fn push_ambiguous_pending_action_message(session: &mut Session) {
    push_controller_message(session, AMBIGUOUS_PENDING_ACTION_MESSAGE);
}

fn record_verified_project_memory(
    session: &mut Session,
    action: &Action,
    _result: &VerifiedActionResult,
) {
    let action_id = action.id.clone();
    match &action.request {
        ActionRequest::CreateDirectory(create_directory) => {
            session.record_verified_folder_reference(VerifiedFolderReference {
                path: resolve_project_path(&session.project_root, &create_directory.target_path),
                source_action_id: action_id,
            });
        }
        ActionRequest::CreateFile(create_file) if is_markdown_path(&create_file.target_path) => {
            record_verified_plan_memory(session, &action_id, &create_file.target_path);
        }
        ActionRequest::OverwriteFile(overwrite_file)
            if is_markdown_path(&overwrite_file.target_path) =>
        {
            record_verified_plan_memory(session, &action_id, &overwrite_file.target_path);
        }
        ActionRequest::ShellCommand(shell_command) => {
            for path in verified_shell_expected_directories(shell_command) {
                session.record_verified_folder_reference(VerifiedFolderReference {
                    path,
                    source_action_id: action_id.clone(),
                });
            }

            if let Some(path) = shell_command
                .expected_file
                .as_ref()
                .filter(|path| is_markdown_path(path))
                .cloned()
                .or_else(|| {
                    shell_command
                        .expected_files
                        .iter()
                        .find(|path| is_markdown_path(path))
                        .cloned()
                })
            {
                session.record_verified_plan_reference(VerifiedPlanReference {
                    project_root: path
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| session.project_root.clone()),
                    path,
                    source_action_id: action_id,
                });
            }
        }
        _ => {}
    }
}

fn verified_shell_expected_directories(shell_command: &ShellCommandAction) -> Vec<PathBuf> {
    let mut expected_directories = Vec::new();
    if let Some(path) = shell_command.expected_directory.clone() {
        expected_directories.push(path);
    }
    expected_directories.extend(shell_command.expected_directories.iter().cloned());
    dedupe_paths(expected_directories)
}

fn record_verified_plan_memory(session: &mut Session, action_id: &str, target_path: &Path) {
    let path = resolve_project_path(&session.project_root, target_path);
    let project_root = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| session.project_root.clone());
    session.record_verified_plan_reference(VerifiedPlanReference {
        path,
        project_root,
        source_action_id: action_id.to_string(),
    });
}

fn verify_expected_shell_effect(
    action: &ShellCommandAction,
    mut result: VerifiedActionResult,
) -> Result<VerifiedActionResult, String> {
    let mut expected_directories = Vec::new();
    if let Some(expected_directory) = action.expected_directory.as_ref() {
        expected_directories.push(expected_directory.clone());
    }
    expected_directories.extend(action.expected_directories.iter().cloned());
    let expected_directories = dedupe_paths(expected_directories);

    let mut expected_files = Vec::new();
    if let Some(expected_file) = action.expected_file.as_ref() {
        expected_files.push(expected_file.clone());
    }
    expected_files.extend(action.expected_files.iter().cloned());
    let expected_files = dedupe_paths(expected_files);

    if expected_directories.is_empty() && expected_files.is_empty() {
        return Ok(result);
    }

    let missing_directories = expected_directories
        .iter()
        .filter(|expected_directory| !expected_directory.is_dir())
        .cloned()
        .collect::<Vec<_>>();
    if !missing_directories.is_empty() {
        return Err(format!(
            "expected directories were not created: {}",
            display_path_list(&missing_directories)
        ));
    }

    let missing_files = expected_files
        .iter()
        .filter(|expected_file| !expected_file.is_file())
        .cloned()
        .collect::<Vec<_>>();
    if !missing_files.is_empty() {
        return Err(format!(
            "expected files were not created: {}",
            display_path_list(&missing_files)
        ));
    }

    if let VerifiedActionResult::Shell(shell) = &mut result {
        let mut effects = Vec::new();
        if expected_directories.len() == 1 {
            effects.push(format!(
                "verified directory exists: {}",
                expected_directories[0].display()
            ));
        } else if !expected_directories.is_empty() {
            effects.push(format!(
                "verified directories exist: {}",
                display_path_list(&expected_directories)
            ));
        }
        if expected_files.len() == 1 {
            effects.push(format!(
                "verified file exists: {}",
                expected_files[0].display()
            ));
        } else if !expected_files.is_empty() {
            effects.push(format!(
                "verified files exist: {}",
                display_path_list(&expected_files)
            ));
        }
        shell.verified_effect = Some(effects.join("; "));
    }

    Ok(result)
}

const RECENT_CONVERSATION_LINE_LIMIT: usize = 8;
const RECENT_CONVERSATION_BYTE_LIMIT: usize = 1_600;
const RECENT_CONVERSATION_LINE_BYTE_LIMIT: usize = 360;
const VERIFIED_MEMORY_BYTE_LIMIT: usize = 1_200;
const VERIFIED_MEMORY_LINE_BYTE_LIMIT: usize = 320;
const VERIFIED_FOLDER_MEMORY_ENTRY_LIMIT: usize = 4;

fn provider_prompt_with_context(session: &mut Session, input: &str) -> String {
    let max_window_tokens = session.context_accounting().max_window_tokens;
    let recent_conversation = recent_conversation_prompt(session);
    let verified_memory = verified_memory_prompt(session, input);
    let local_context_budget = context_budget_tokens(max_window_tokens).saturating_sub(
        prompt_extension_tokens(recent_conversation.as_deref(), verified_memory.as_deref()),
    );
    let bundle = ContextBundle::from_default_local_files_with_budget(
        &session.project_root,
        &session.cwd,
        max_window_tokens,
        local_context_budget,
    );
    session.set_context_accounting(bundle.accounting.clone());
    bundle.prompt_for_with_recent_conversation_and_verified_memory(
        recent_conversation.as_deref(),
        verified_memory.as_deref(),
        input,
    )
}

fn model_first_provider_prompt_with_context(session: &mut Session, input: &str) -> String {
    let prompt = provider_prompt_with_context(session, input);
    format!("{MODEL_FIRST_TOOL_CONTRACT}\n\n{prompt}")
}

const MODEL_FIRST_TOOL_CONTRACT: &str = "Model-first tool contract selected by Elgar controller:\n- For requests to create, implement, or make project files, return create_directory/create_file tool calls for actual filesystem changes; do not answer with prose-only file contents or claim success.\n- If target, scope, verified memory, or safe next step is ambiguous, use ask_guidance with one concise question instead of guessing.\n- Multiple safe create_file/create_directory calls are allowed for multi-file project creation.\n- Shell, overwrite, patch, delete, and move are review-gated. Do not use shell commands for package installation or project setup in this flow.\n- When verified memory names a latest folder, same folder, or plan project root, target project files inside that verified folder/root.";

fn prompt_extension_tokens(
    recent_conversation: Option<&str>,
    verified_memory: Option<&str>,
) -> u64 {
    [recent_conversation, verified_memory]
        .into_iter()
        .flatten()
        .map(|section| (section.len() as u64).div_ceil(4))
        .sum()
}

fn verified_memory_prompt(session: &mut Session, input: &str) -> Option<String> {
    let need = VerifiedMemoryNeed::from_input(input);
    if !need.any() {
        session.set_latest_provider_prompt_memory_selection(None);
        return None;
    }

    let selection = {
        let memory = session.project_memory();
        let mut selection = VerifiedMemoryPromptSelection::default();

        if need.folder {
            select_verified_folder_memory(memory, &mut selection);
        }
        if need.plan {
            select_verified_plan_memory(memory, &mut selection);
            select_structured_plan_memory(memory, &mut selection);
        }

        selection
    };

    if selection.is_empty() {
        session.set_latest_provider_prompt_memory_selection(None);
        return None;
    }

    let mut lines = Vec::new();
    let mut selected_facts = Vec::new();
    let mut omitted_facts = Vec::new();
    for item in selection
        .selected_items
        .into_iter()
        .chain(selection.omitted_items)
    {
        if push_verified_memory_line(&mut lines, item.line) {
            match item.fact {
                VerifiedMemoryPromptFact::Selected(fact) => selected_facts.push(fact),
                VerifiedMemoryPromptFact::Omitted(fact) => omitted_facts.push(fact),
            }
        }
    }

    session.set_latest_provider_prompt_memory_selection(Some(ProviderPromptMemorySelection::new(
        selected_facts,
        omitted_facts,
    )));

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

#[derive(Debug, Default)]
struct VerifiedMemoryPromptSelection {
    selected_items: Vec<VerifiedMemoryPromptItem>,
    omitted_items: Vec<VerifiedMemoryPromptItem>,
}

impl VerifiedMemoryPromptSelection {
    fn is_empty(&self) -> bool {
        self.selected_items.is_empty() && self.omitted_items.is_empty()
    }

    fn select(
        &mut self,
        line: String,
        kind: &'static str,
        path: std::path::PathBuf,
        project_root: Option<std::path::PathBuf>,
        source_action_id: String,
    ) {
        self.selected_items.push(VerifiedMemoryPromptItem {
            line,
            fact: VerifiedMemoryPromptFact::Selected(ProviderPromptMemorySelectedFact::new(
                kind,
                path,
                project_root,
                source_action_id,
            )),
        });
    }

    fn omit(
        &mut self,
        line: String,
        kind: &'static str,
        path: std::path::PathBuf,
        project_root: Option<std::path::PathBuf>,
        source_action_id: String,
        reason: &'static str,
    ) {
        self.omitted_items.push(VerifiedMemoryPromptItem {
            line,
            fact: VerifiedMemoryPromptFact::Omitted(ProviderPromptMemoryOmittedFact::new(
                kind,
                path,
                project_root,
                source_action_id,
                reason,
            )),
        });
    }
}

#[derive(Debug)]
struct VerifiedMemoryPromptItem {
    line: String,
    fact: VerifiedMemoryPromptFact,
}

#[derive(Debug)]
enum VerifiedMemoryPromptFact {
    Selected(ProviderPromptMemorySelectedFact),
    Omitted(ProviderPromptMemoryOmittedFact),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VerifiedMemoryNeed {
    folder: bool,
    plan: bool,
}

impl VerifiedMemoryNeed {
    fn from_input(input: &str) -> Self {
        let lower = input.to_ascii_lowercase();
        let reference = contains_any(
            &lower,
            &[
                "that ",
                "this ",
                "the folder",
                "the directory",
                "the plan",
                "the project",
                "same folder",
                "same directory",
                "inside the folder you created",
                "folder you created",
                "rest of the project",
                "go ahead and make the files",
                "make the files",
                "implement the plan",
                "where is",
                "where did you put",
                "what path",
                "path did you create",
                "dont see",
                "don't see",
                "continue",
                "next step",
                "run it",
                "execute it",
            ],
        );
        let folder = reference
            && contains_any(
                &lower,
                &[
                    "folder",
                    "directory",
                    "there",
                    "where is",
                    "where did you put",
                    "what path",
                    "path did you create",
                    "dont see",
                    "don't see",
                    "same folder",
                    "same directory",
                    "inside the folder you created",
                    "folder you created",
                    "project",
                    "files",
                    "implement the plan",
                ],
            );
        let plan = reference
            && contains_any(
                &lower,
                &[
                    "plan",
                    "implement",
                    "execute",
                    "run it",
                    "continue",
                    "next step",
                    "project",
                    "make the files",
                    "rest of the project",
                ],
            );

        Self { folder, plan }
    }

    fn any(self) -> bool {
        self.folder || self.plan
    }
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn select_verified_folder_memory(
    memory: &crate::session::ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(latest_reference) = memory.verified_folders.last() else {
        return;
    };

    if !latest_reference.path.is_dir() {
        selection.omit(
            format!(
                "omitted missing verified folder: {} (source action {})",
                latest_reference.path.display(),
                latest_reference.source_action_id
            ),
            "verified_folder",
            latest_reference.path.clone(),
            None,
            latest_reference.source_action_id.clone(),
            "missing",
        );
        return;
    }

    let mut selected_count = 0;
    for reference in memory.verified_folders.iter().rev() {
        if reference.path.is_dir() {
            selection.select(
                format!(
                    "verified folder: {} (source action {})",
                    reference.path.display(),
                    reference.source_action_id
                ),
                "verified_folder",
                reference.path.clone(),
                None,
                reference.source_action_id.clone(),
            );
            selected_count += 1;
            if selected_count >= VERIFIED_FOLDER_MEMORY_ENTRY_LIMIT {
                return;
            }
            continue;
        }

        selection.omit(
            format!(
                "omitted missing verified folder: {} (source action {})",
                reference.path.display(),
                reference.source_action_id
            ),
            "verified_folder",
            reference.path.clone(),
            None,
            reference.source_action_id.clone(),
            "missing",
        );
    }
}

fn select_verified_plan_memory(
    memory: &crate::session::ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(reference) = memory.verified_plans.last() else {
        return;
    };

    if !reference.path.is_file() {
        selection.omit(
            format!(
                "omitted missing verified plan: {} (source action {})",
                reference.path.display(),
                reference.source_action_id
            ),
            "verified_plan",
            reference.path.clone(),
            Some(reference.project_root.clone()),
            reference.source_action_id.clone(),
            "missing",
        );
        return;
    }
    if !reference.project_root.is_dir() {
        selection.omit(
            format!(
                "omitted verified plan with missing project root: {} -> {} (source action {})",
                reference.path.display(),
                reference.project_root.display(),
                reference.source_action_id
            ),
            "verified_plan",
            reference.path.clone(),
            Some(reference.project_root.clone()),
            reference.source_action_id.clone(),
            "missing",
        );
        return;
    }

    selection.select(
        format!(
            "latest verified plan: {} (project root {}; source action {})",
            reference.path.display(),
            reference.project_root.display(),
            reference.source_action_id
        ),
        "verified_plan",
        reference.path.clone(),
        Some(reference.project_root.clone()),
        reference.source_action_id.clone(),
    );
}

fn select_structured_plan_memory(
    memory: &crate::session::ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(plan) = memory.structured_plans.last() else {
        return;
    };
    let source_action = plan.source_action_id.as_deref().unwrap_or("(none)");
    if !plan.source_plan_path.is_file() {
        selection.omit(
            format!(
                "omitted structured plan with missing plan file: {} (source action {})",
                plan.source_plan_path.display(),
                source_action
            ),
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            source_action.to_string(),
            "missing",
        );
        return;
    }
    if !plan.project_root.is_dir() {
        selection.omit(
            format!(
                "omitted structured plan with missing project root: {} -> {} (source action {})",
                plan.source_plan_path.display(),
                plan.project_root.display(),
                source_action
            ),
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            source_action.to_string(),
            "missing",
        );
        return;
    }

    selection.select(
        format!(
            "latest structured plan: status {:?}, stage {}, expected dirs {}, expected files {}, plan {}, project root {}, source action {}",
            plan.status,
            plan.stage,
            plan.expected_directories.len(),
            plan.expected_files.len(),
            plan.source_plan_path.display(),
            plan.project_root.display(),
            source_action
        ),
        "structured_plan",
        plan.source_plan_path.clone(),
        Some(plan.project_root.clone()),
        source_action.to_string(),
    );
}

fn push_verified_memory_line(lines: &mut Vec<String>, line: String) -> bool {
    let line = truncate_line(&line, VERIFIED_MEMORY_LINE_BYTE_LIMIT);
    let current_bytes = conversation_bytes(lines);
    let line_bytes = line.len() + 1;
    if current_bytes + line_bytes <= VERIFIED_MEMORY_BYTE_LIMIT {
        lines.push(line);
        true
    } else if lines.is_empty() {
        lines.push(truncate_line(&line, VERIFIED_MEMORY_BYTE_LIMIT));
        true
    } else {
        false
    }
}

fn recent_conversation_prompt(session: &Session) -> Option<String> {
    let events = session.events();
    let events = match events.last() {
        Some(Event::UserMessage(_)) => &events[..events.len().saturating_sub(1)],
        _ => events,
    };

    let mut lines = events
        .iter()
        .filter_map(recent_conversation_line)
        .map(|line| truncate_line(&line, RECENT_CONVERSATION_LINE_BYTE_LIMIT))
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    if lines.len() > RECENT_CONVERSATION_LINE_LIMIT {
        lines = lines[lines.len() - RECENT_CONVERSATION_LINE_LIMIT..].to_vec();
    }

    while conversation_bytes(&lines) > RECENT_CONVERSATION_BYTE_LIMIT && lines.len() > 1 {
        lines.remove(0);
    }

    if conversation_bytes(&lines) > RECENT_CONVERSATION_BYTE_LIMIT {
        lines[0] = truncate_line(&lines[0], RECENT_CONVERSATION_BYTE_LIMIT);
    }

    Some(lines.join("\n"))
}

fn recent_conversation_line(event: &Event) -> Option<String> {
    match event {
        Event::UserMessage(user) => Some(format!("user: {}", compact_prompt_text(&user.content))),
        Event::AssistantMessage(message) => Some(format!(
            "assistant({}): {}",
            assistant_source_label(message.source),
            compact_prompt_text(&message.content)
        )),
        Event::ActionProposed(action) => Some(format!(
            "controller action proposed: {:?} {} - {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)"),
            compact_prompt_text(&action.summary)
        )),
        Event::ActionApproved(action) => Some(format!(
            "controller action approved: {:?} {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)")
        )),
        Event::ActionRejected(action) => Some(format!(
            "controller action rejected: {:?} {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)")
        )),
        Event::ActionApplied(action) => Some(format!(
            "controller verified action applied: {:?} {:?}",
            action.action_kind, action.result
        )),
        Event::ActionFailed(action) => Some(format!(
            "controller action failed: {:?} - {}",
            action.action_kind,
            compact_prompt_text(&action.reason)
        )),
        Event::Error(error) => Some(format!(
            "controller error: {}",
            compact_prompt_text(&error.message)
        )),
        Event::ProviderStarted(_) | Event::ProviderFinished(_) => None,
    }
}

fn assistant_source_label(source: AssistantMessageSource) -> &'static str {
    match source {
        AssistantMessageSource::Controller => "controller",
        AssistantMessageSource::Provider => "provider",
    }
}

fn compact_prompt_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn conversation_bytes(lines: &[String]) -> usize {
    lines.iter().map(|line| line.len() + 1).sum()
}

fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        return line.to_string();
    }

    let suffix = "...";
    let max_content = max_bytes.saturating_sub(suffix.len());
    let mut end = max_content.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &line[..end], suffix)
}

fn set_provider_metrics_metadata(
    session: &mut Session,
    request: &crate::provider::ProviderRequestMetadata,
    metrics: crate::event::ProviderMetrics,
) {
    let mut metadata = ProviderMetadata::new(request.provider.clone());
    metadata.model = request.model.clone();
    metadata.request_id = Some(request.request_id.clone());
    metadata.metrics = Some(metrics);
    session.set_provider_metadata(metadata);
}

fn next_action_id(session: &Session) -> String {
    format!("action-{}", session.actions().len() + 1)
}

fn action_target_label(action: &Action) -> String {
    match &action.request {
        crate::action::ActionRequest::CreateFile(create_file) => {
            create_file.target_path.display().to_string()
        }
        request => request.approval_target(),
    }
}

fn markdown_plan_prompt(input: &str, target_path: &std::path::Path) -> String {
    format!(
        "Create concise Markdown content for `{}`. Return only Markdown content, no code fences, no approval claims, and no claim that a file was written.\n\nRequest: {}",
        target_path.display(),
        input.trim()
    )
}

fn normalize_markdown_plan_contents(text: &str) -> String {
    let text = strip_markdown_code_fence(text.trim()).trim();
    if text.is_empty() {
        String::new()
    } else {
        format!("{text}\n")
    }
}

fn strip_markdown_code_fence(text: &str) -> &str {
    let Some(after_opening) = text.strip_prefix("```") else {
        return text;
    };
    let after_opening = after_opening
        .split_once('\n')
        .map(|(_language, body)| body)
        .unwrap_or(after_opening);
    after_opening
        .rsplit_once("```")
        .map(|(body, _closing)| body)
        .unwrap_or(after_opening)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ControllerOwnedProjectPlanProposal {
    project_root: PathBuf,
    plan_path: PathBuf,
    contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControllerOwnedProjectKind {
    React,
    ReactTs,
    Generic,
}

fn controller_owned_project_plan_proposal(
    input: &str,
    session: &Session,
) -> Result<Option<ControllerOwnedProjectPlanProposal>, String> {
    if !is_project_creation_request(input)
        && !is_controller_owned_explicit_project_plan_request(input)
    {
        return Ok(None);
    }

    let kind = ControllerOwnedProjectKind::from_input(input);
    let project_root = project_creation_root(input, session, kind)?;
    let plan_path = project_root.join(controller_owned_plan_file_name(kind));
    let contents = controller_owned_project_plan_contents(kind, &project_root);

    Ok(Some(ControllerOwnedProjectPlanProposal {
        project_root,
        plan_path,
        contents,
    }))
}

impl ControllerOwnedProjectKind {
    fn from_input(input: &str) -> Self {
        let normalized = input.to_ascii_lowercase();
        if normalized.contains("react") && mentions_typescript(&normalized) {
            Self::ReactTs
        } else if normalized.contains("react") {
            Self::React
        } else {
            Self::Generic
        }
    }
}

fn is_controller_owned_explicit_project_plan_request(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    let asks_for_plan = normalized.contains(" plan ")
        || normalized.starts_with("create a plan ")
        || normalized.starts_with("create plan ")
        || normalized.starts_with("please create a plan ")
        || normalized.starts_with("please write a plan ");

    asks_for_plan && normalized.contains("react")
}

fn mentions_typescript(input: &str) -> bool {
    input.contains(" ts") || input.contains("typescript") || input.contains("type script")
}

fn project_creation_root(
    input: &str,
    session: &Session,
    kind: ControllerOwnedProjectKind,
) -> Result<PathBuf, String> {
    if let Some(path) = explicit_project_creation_path(input) {
        return Ok(path);
    }

    if references_prior_folder(input) || is_prior_project_execution_request(input) {
        return latest_existing_verified_directory_reference(session);
    }

    let folder_name = project_creation_folder_name(input)
        .unwrap_or_else(|| default_project_folder_name(kind).to_string());
    if input.to_ascii_lowercase().contains("desktop") {
        let Some(home) = home_dir() else {
            return Err(
                "Desktop project request needs HOME to resolve the Desktop path.".to_string(),
            );
        };
        return Ok(home.join("Desktop").join(folder_name));
    }

    Ok(session.project_root.join(folder_name))
}

fn missing_project_plan_proposal_for_latest_folder(
    input: &str,
    session: &Session,
) -> Result<ControllerOwnedProjectPlanProposal, String> {
    let kind = ControllerOwnedProjectKind::from_input(input);
    let project_root = latest_existing_verified_directory_reference(session)?;
    let plan_path = project_root.join(controller_owned_plan_file_name(kind));
    let contents = controller_owned_project_plan_contents(kind, &project_root);

    Ok(ControllerOwnedProjectPlanProposal {
        project_root,
        plan_path,
        contents,
    })
}

fn explicit_project_creation_path(input: &str) -> Option<PathBuf> {
    for delimiter in [" at ", " under ", " in "] {
        let Some((_head, location)) = split_ascii_case_once(input, delimiter) else {
            continue;
        };
        let location = trim_request_punctuation(trim_directory_location_marker(location));
        let Some(token) = directory_target_token(location) else {
            continue;
        };
        if let Some(path) = parse_external_directory_target(&token) {
            return Some(path);
        }
    }

    None
}

fn project_creation_folder_name(input: &str) -> Option<String> {
    for marker in [
        "folder you need to create called ",
        "folder you need to create named ",
        "project called ",
        "project named ",
        "app called ",
        "app named ",
        "application called ",
        "application named ",
        "folder called ",
        "folder named ",
        "directory called ",
        "directory named ",
    ] {
        if let Some((_head, rest)) = split_ascii_case_once(input, marker) {
            return directory_target_token(rest);
        }
    }

    None
}

fn default_project_folder_name(kind: ControllerOwnedProjectKind) -> &'static str {
    match kind {
        ControllerOwnedProjectKind::React => "react-project",
        ControllerOwnedProjectKind::ReactTs => "react-ts-project",
        ControllerOwnedProjectKind::Generic => "project",
    }
}

fn controller_owned_plan_file_name(kind: ControllerOwnedProjectKind) -> &'static str {
    match kind {
        ControllerOwnedProjectKind::React => "react-project-plan.md",
        ControllerOwnedProjectKind::ReactTs => "react-ts-project-plan.md",
        ControllerOwnedProjectKind::Generic => "project-plan.md",
    }
}

fn controller_owned_project_plan_contents(
    kind: ControllerOwnedProjectKind,
    project_root: &Path,
) -> String {
    match kind {
        ControllerOwnedProjectKind::React => format!(
            "# React Project Plan\n\nProject root: {}\n\n- Create the project folder.\n- Add a Vite-style React TypeScript scaffold with `index.html`, `package.json`, `tsconfig.json`, `vite.config.ts`, and `src/` files.\n- Defer package installation. A later `npm install` or package-manager command must be proposed and approved separately before dependencies are downloaded.\n",
            project_root.display()
        ),
        ControllerOwnedProjectKind::ReactTs => format!(
            "# React TS Project Plan\n\nProject root: {}\n\n- Create the project folder.\n- Add a Vite-style React TypeScript scaffold with `index.html`, `package.json`, `tsconfig.json`, `vite.config.ts`, and `src/` files.\n- Defer package installation. A later `npm install` or package-manager command must be proposed and approved separately before dependencies are downloaded.\n",
            project_root.display()
        ),
        ControllerOwnedProjectKind::Generic => format!(
            "# Project Plan\n\nProject root: {}\n\n- Create the project folder.\n- Add a small local scaffold.\n- Defer network-heavy dependency installation until a separate shell command is proposed and approved.\n",
            project_root.display()
        ),
    }
}

fn parse_markdown_plan_target(
    input: &str,
    session: &Session,
) -> Result<std::path::PathBuf, String> {
    let explicit_target = explicit_markdown_target(input);
    let slug = markdown_plan_slug(input);
    if references_prior_folder(input) {
        let folder = latest_existing_verified_directory_reference(session)?;
        return Ok(match explicit_target {
            Some(target) => markdown_target_inside_folder(folder, target),
            None => folder.join(format!("{slug}-plan.md")),
        });
    }

    if let Some(explicit_target) = explicit_target {
        return Ok(expand_markdown_target(explicit_target));
    }

    if let Some(folder) = markdown_plan_target_folder(input, session)? {
        return Ok(folder.join(format!("{slug}-plan.md")));
    }

    if input.to_ascii_lowercase().contains("desktop") {
        if let Some(home) = home_dir() {
            return Ok(home.join("Desktop").join(format!("{slug}-plan.md")));
        }
    }

    Ok(std::path::PathBuf::from(format!("{slug}-plan.md")))
}

fn markdown_plan_target_folder(input: &str, session: &Session) -> Result<Option<PathBuf>, String> {
    for delimiter in [" inside ", " under ", " at ", " in ", " on "] {
        let Some((_head, location)) = split_ascii_case_once(input, delimiter) else {
            continue;
        };
        let location = trim_request_punctuation(trim_directory_location_marker(location));
        if is_prior_folder_location(location) {
            return latest_existing_verified_directory_reference(session).map(Some);
        }
        let location = match location.to_ascii_lowercase().as_str() {
            "that folder" | "this folder" | "the folder" => {
                return latest_existing_verified_directory_reference(session).map(Some);
            }
            "my desktop" | "the desktop" | "desktop" => "Desktop",
            "project" | "project root" | "repo" | "repo root" => {
                return Ok(Some(session.project_root.clone()));
            }
            _ => location,
        };
        let Some(location) = directory_target_token(location) else {
            continue;
        };
        if let Some(path) = parse_external_directory_target(&location) {
            return Ok(Some(path));
        }
    }

    Ok(None)
}

fn markdown_target_inside_folder(folder: PathBuf, target: PathBuf) -> PathBuf {
    let target = expand_markdown_target(target);
    if target.is_absolute() {
        target
    } else {
        folder.join(target)
    }
}

fn references_prior_folder(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    normalized.contains("that folder")
        || normalized.contains("this folder")
        || normalized.contains("the folder")
        || normalized.contains("same folder")
        || normalized.contains("same directory")
        || normalized.contains("folder you created")
        || normalized.contains("directory you created")
        || normalized.contains("folder you just created")
        || normalized.contains("folder we just created")
}

fn expand_markdown_target(target: std::path::PathBuf) -> std::path::PathBuf {
    let Some(target_text) = target.to_str() else {
        return target;
    };
    parse_external_directory_target(target_text).unwrap_or(target)
}

fn explicit_markdown_target(input: &str) -> Option<std::path::PathBuf> {
    input.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|character: char| {
            character.is_ascii_punctuation() && !matches!(character, '.' | '/' | '_' | '-')
        });
        token
            .to_ascii_lowercase()
            .ends_with(".md")
            .then(|| std::path::PathBuf::from(token))
    })
}

fn markdown_plan_slug(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let subject = [" for ", " about ", " to "]
        .iter()
        .find_map(|delimiter| lower.rsplit_once(delimiter).map(|(_head, tail)| tail))
        .unwrap_or(&lower);
    let ignored = [
        "a", "an", "and", "at", "build", "create", "desktop", "draft", "file", "folder", "in",
        "inside", "make", "markdown", "md", "my", "on", "plan", "please", "that", "the", "this",
        "under", "use", "using", "with", "write",
    ];
    let words: Vec<&str> = subject
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .filter(|word| !ignored.contains(word))
        .take(4)
        .collect();

    if words.is_empty() {
        "plan".to_string()
    } else {
        words.join("-")
    }
}

fn parse_write_file_target(input: &str) -> Option<std::path::PathBuf> {
    let trimmed = strip_action_request_prefixes(input.trim());
    for prefix in [
        "create a file ",
        "create file ",
        "write a file ",
        "write file ",
        "create ",
        "write ",
    ] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            return rest
                .split_whitespace()
                .next()
                .filter(|target| !target.is_empty())
                .map(std::path::PathBuf::from);
        }
    }

    None
}

fn parse_patch_file_request(input: &str) -> Option<(std::path::PathBuf, String, String)> {
    let trimmed = input.trim();
    for prefix in ["edit file ", "patch file ", "edit ", "patch "] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            let (target, edit) = split_first_token(rest)?;
            let edit = edit.trim_start();
            let edit = strip_ascii_case_prefix(edit, "replace ")?;
            let (find, replace) = split_ascii_case_once(edit, " with ")?;
            if find.is_empty() {
                return None;
            }
            return Some((
                std::path::PathBuf::from(target),
                find.to_string(),
                replace.to_string(),
            ));
        }
    }

    None
}

fn parse_overwrite_file_request(input: &str) -> Option<(std::path::PathBuf, String)> {
    let trimmed = input.trim();
    for prefix in ["overwrite file ", "overwrite "] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            let (target, contents) = split_first_token(rest)?;
            let contents = contents.trim_start();
            let contents = strip_ascii_case_prefix(contents, "with ").unwrap_or(contents);
            return Some((std::path::PathBuf::from(target), contents.to_string()));
        }
    }

    None
}

fn parse_delete_file_target(input: &str) -> Option<std::path::PathBuf> {
    let trimmed = input.trim();
    for prefix in ["delete file ", "remove file ", "delete ", "remove "] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            return rest
                .split_whitespace()
                .next()
                .filter(|target| !target.is_empty())
                .map(std::path::PathBuf::from);
        }
    }

    None
}

fn parse_move_file_request(input: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let trimmed = input.trim();
    for prefix in ["move file ", "rename file ", "move ", "rename "] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            let (source, target) = split_ascii_case_once(rest, " to ")?;
            let source = source.trim();
            let target = target.trim();
            if source.is_empty() || target.is_empty() {
                return None;
            }
            return Some((source.into(), target.into()));
        }
    }

    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedCreateDirectoryTarget {
    ProjectRelative(PathBuf),
    ShellDirectory(PathBuf),
    ShellDirectories(Vec<PathBuf>),
}

fn parse_create_directory_target(input: &str) -> Option<ParsedCreateDirectoryTarget> {
    let trimmed = strip_controller_action_request_prefixes(input.trim());
    for prefix in [
        "create directory ",
        "create a directory ",
        "create dir ",
        "create folder ",
        "create a folder ",
        "make directory ",
        "make a directory ",
        "make dir ",
        "make folder ",
        "make a folder ",
        "can you create a directory ",
        "can you create a folder ",
        "can you make a directory ",
        "can you make a folder ",
        "please create a directory ",
        "please create a folder ",
        "please make a directory ",
        "please make a folder ",
        "mkdir ",
    ] {
        if let Some(rest) = strip_ascii_case_prefix(trimmed, prefix) {
            return parse_create_directory_target_rest(rest);
        }
    }

    None
}

fn strip_controller_action_request_prefixes(input: &str) -> &str {
    let mut stripped = input.trim_start();

    loop {
        let next = strip_ascii_case_prefix(stripped, "can you ")
            .or_else(|| strip_ascii_case_prefix(stripped, "could you "))
            .or_else(|| strip_ascii_case_prefix(stripped, "would you "))
            .or_else(|| strip_ascii_case_prefix(stripped, "please "))
            .or_else(|| strip_ascii_case_prefix(stripped, "okay "))
            .or_else(|| strip_ascii_case_prefix(stripped, "ok "));

        match next {
            Some(value) => stripped = value.trim_start(),
            None => return stripped,
        }
    }
}

fn parse_create_directory_plan_followup_target(
    session: &Session,
    input: &str,
) -> Result<Option<ParsedCreateDirectoryTarget>, String> {
    if !is_create_directory_plan_followup(input) {
        return Ok(None);
    }

    let Some(plan_path) = latest_verified_markdown_plan_file(session) else {
        return Ok(None);
    };
    let base_path = parse_directory_plan_followup_base(input, session)?
        .or_else(|| verified_plan_project_root(session, &plan_path))
        .ok_or_else(|| "The verified Markdown plan has no executable target folder.".to_string())?;
    let plan = std::fs::read_to_string(&plan_path).map_err(|_| {
        format!(
            "The latest verified Markdown plan is missing: {}. Recreate the plan before using it.",
            plan_path.display()
        )
    })?;
    let Some(relative_paths) = extract_directory_plan_relative_paths(&plan) else {
        return Ok(None);
    };
    let target_paths = relative_paths
        .into_iter()
        .map(|relative_path| base_path.join(relative_path))
        .collect::<Vec<_>>();

    Ok(Some(ParsedCreateDirectoryTarget::ShellDirectories(
        target_paths,
    )))
}

fn is_create_directory_plan_followup(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    let asks_to_create = normalized.contains("create ")
        || normalized.contains("make ")
        || normalized.contains("generate ");
    let references_prior_plan = normalized.contains("this plan")
        || normalized.contains("that plan")
        || normalized.contains("the plan")
        || normalized.contains("these folders")
        || normalized.contains("those folders")
        || normalized.contains("the folders");
    let has_location_or_folder_intent = normalized.contains("desktop")
        || normalized.contains("folder")
        || normalized.contains("directory")
        || normalized.contains(" at ")
        || normalized.contains(" in ")
        || normalized.contains(" under ")
        || normalized.contains(" inside ");

    asks_to_create && references_prior_plan && has_location_or_folder_intent
}

fn parse_directory_plan_followup_base(
    input: &str,
    session: &Session,
) -> Result<Option<PathBuf>, String> {
    let normalized = input.trim().to_ascii_lowercase();
    for delimiter in [" under ", " inside ", " at ", " in ", " on "] {
        if let Some((_request, location)) = split_ascii_case_once(input, delimiter) {
            let location = trim_directory_location_marker(location);
            let location = trim_request_punctuation(location);
            if is_prior_folder_location(location) {
                return latest_existing_verified_directory_reference(session).map(Some);
            }
            let location = match location.to_ascii_lowercase().as_str() {
                "my desktop" | "the desktop" | "desktop" => "Desktop",
                "that folder" | "this folder" | "the folder" => {
                    return latest_existing_verified_directory_reference(session).map(Some);
                }
                "project" | "project root" | "repo" | "repo root" => {
                    return Ok(Some(session.project_root.clone()));
                }
                _ => location,
            };
            let Some(location) = directory_target_token(location) else {
                continue;
            };
            if let Some(path) = parse_external_directory_target(&location) {
                return Ok(Some(path));
            }
        }
    }

    if normalized.contains("desktop") {
        return Ok(home_dir().map(|home| home.join("Desktop")));
    }

    Ok(None)
}

fn is_prior_folder_location(location: &str) -> bool {
    let location = trim_location_filler_suffix(location).to_ascii_lowercase();
    matches!(
        location.as_str(),
        "that folder"
            | "this folder"
            | "the folder"
            | "folder you created"
            | "directory you created"
            | "folder you just created"
            | "folder we just created"
    )
}

fn trim_location_filler_suffix(input: &str) -> &str {
    let mut trimmed = trim_request_punctuation(input.trim());
    loop {
        let lower = trimmed.to_ascii_lowercase();
        let Some(suffix) = [" please", " thanks", " thank you"]
            .iter()
            .find(|suffix| lower.ends_with(**suffix))
        else {
            return trimmed;
        };
        trimmed = trim_request_punctuation(trimmed[..trimmed.len() - suffix.len()].trim());
    }
}

fn latest_existing_verified_directory_reference(session: &Session) -> Result<PathBuf, String> {
    let Some(path) = latest_verified_directory_reference(session) else {
        return Err(
            "No verified folder is available. Create the folder or provide an explicit path."
                .to_string(),
        );
    };

    if path.is_dir() {
        return Ok(path);
    }

    Err(format!(
        "The latest verified folder is missing: {}. Recreate the folder or provide an explicit path.",
        path.display()
    ))
}

fn latest_verified_directory_reference(session: &Session) -> Option<PathBuf> {
    if let Some(reference) = session.project_memory().latest_verified_folder() {
        return Some(reference.path.clone());
    }

    session
        .actions()
        .iter()
        .rev()
        .filter_map(|record| verified_directory_reference(record, &session.project_root))
        .next()
}

fn verified_directory_reference(record: &ActionRecord, project_root: &Path) -> Option<PathBuf> {
    record.verified_result.as_ref()?;
    match &record.action.request {
        ActionRequest::CreateDirectory(action) => {
            Some(resolve_project_path(project_root, &action.target_path))
        }
        ActionRequest::ShellCommand(action) => action
            .expected_directory
            .clone()
            .or_else(|| action.expected_directories.first().cloned()),
        _ => None,
    }
}

fn latest_verified_markdown_plan_file(session: &Session) -> Option<PathBuf> {
    if let Some(reference) = session.project_memory().latest_verified_plan() {
        return Some(reference.path.clone());
    }

    session
        .actions()
        .iter()
        .rev()
        .filter_map(|record| verified_markdown_file_reference(record, &session.project_root))
        .next()
}

fn verified_plan_project_root(session: &Session, plan_path: &Path) -> Option<PathBuf> {
    session
        .project_memory()
        .verified_plans
        .iter()
        .rev()
        .find(|reference| reference.path == plan_path && reference.project_root.is_dir())
        .map(|reference| reference.project_root.clone())
        .or_else(|| {
            plan_path
                .parent()
                .filter(|parent| parent.is_dir())
                .map(Path::to_path_buf)
        })
}

fn verified_markdown_file_reference(record: &ActionRecord, project_root: &Path) -> Option<PathBuf> {
    record.verified_result.as_ref()?;
    match &record.action.request {
        ActionRequest::CreateFile(action) if is_markdown_path(&action.target_path) => {
            Some(resolve_project_path(project_root, &action.target_path))
        }
        ActionRequest::OverwriteFile(action) if is_markdown_path(&action.target_path) => {
            Some(resolve_project_path(project_root, &action.target_path))
        }
        ActionRequest::ShellCommand(action) => action
            .expected_file
            .as_ref()
            .filter(|path| is_markdown_path(path))
            .cloned()
            .or_else(|| {
                action
                    .expected_files
                    .iter()
                    .find(|path| is_markdown_path(path))
                    .cloned()
            }),
        _ => None,
    }
}

fn resolve_project_path(project_root: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        target_path.to_path_buf()
    } else {
        project_root.join(target_path)
    }
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}

fn extract_directory_plan_relative_paths(plan: &str) -> Option<Vec<PathBuf>> {
    let entries = plan
        .lines()
        .filter_map(directory_plan_line_entry)
        .collect::<Vec<_>>();
    let root = entries.first()?;
    let mut paths = Vec::new();

    for child in entries.iter().skip(1) {
        if child == root {
            continue;
        }
        paths.push(PathBuf::from(root).join(child));
    }

    if paths.is_empty() {
        paths.push(PathBuf::from(root));
    }

    Some(dedupe_paths(paths))
}

fn directory_plan_line_entry(line: &str) -> Option<String> {
    let before_comment = line.split('#').next()?.trim();
    let token = before_comment
        .split_whitespace()
        .last()?
        .trim_matches('`')
        .trim();
    let token = token.strip_suffix('/')?;
    if !is_safe_relative_plan_path(token) {
        return None;
    }
    Some(token.to_string())
}

fn is_safe_relative_plan_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|component| {
            matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
}

fn parse_create_directory_target_rest(rest: &str) -> Option<ParsedCreateDirectoryTarget> {
    let rest = trim_request_punctuation(rest);
    if let Some(name) = parse_create_directory_desktop_named_target(rest) {
        return Some(ParsedCreateDirectoryTarget::ShellDirectory(
            desktop_directory_target(&name)?,
        ));
    }

    let (rest, targets_desktop) = split_create_directory_desktop_location(rest);

    if let Some((name, location)) = split_create_directory_external_location(rest) {
        let name = directory_name_target(name)?;
        let location = directory_target_token(trim_directory_location_marker(location))?;
        if let Some(location_path) = parse_external_directory_target(&location) {
            return Some(ParsedCreateDirectoryTarget::ShellDirectory(
                location_path.join(name),
            ));
        }
    }

    let target = trim_directory_location_marker(rest);
    let target = trim_request_punctuation(target);
    let target = if targets_desktop {
        directory_name_target(target)?
    } else {
        directory_target_token(trim_directory_name_marker(target))?
    };

    if targets_desktop {
        return Some(ParsedCreateDirectoryTarget::ShellDirectory(
            desktop_directory_target(&target)?,
        ));
    }

    if let Some(path) = parse_external_directory_target(&target) {
        return Some(ParsedCreateDirectoryTarget::ShellDirectory(path));
    }

    Some(ParsedCreateDirectoryTarget::ProjectRelative(PathBuf::from(
        target,
    )))
}

fn parse_create_directory_desktop_named_target(rest: &str) -> Option<String> {
    let rest = trim_request_punctuation(rest);
    for prefix in [
        "in the desktop",
        "on the desktop",
        "in desktop",
        "on desktop",
        "in my desktop",
        "on my desktop",
    ] {
        let Some(after_location) = strip_ascii_case_prefix(rest, prefix) else {
            continue;
        };
        let after_location = after_location.trim();
        if after_location.is_empty() {
            return None;
        }
        return directory_name_target(after_location);
    }

    None
}

fn split_create_directory_desktop_location(rest: &str) -> (&str, bool) {
    for delimiter in [
        " in the desktop",
        " on the desktop",
        " in desktop",
        " on desktop",
        " in my desktop",
        " on my desktop",
    ] {
        if let Some((target, _location)) = split_ascii_case_once(rest, delimiter) {
            return (target, true);
        }
    }

    (rest, false)
}

fn trim_directory_name_marker(input: &str) -> &str {
    strip_directory_name_marker(input).0
}

fn strip_directory_name_marker(input: &str) -> (&str, bool) {
    let input = input.trim();
    for prefix in [
        "and call it ",
        "and name it ",
        "and called ",
        "and named ",
        "called ",
        "named ",
        "call it ",
        "name it ",
    ] {
        if let Some(rest) = strip_ascii_case_prefix(input, prefix) {
            return (rest.trim(), true);
        }
    }
    (input, false)
}

fn trim_directory_location_marker(input: &str) -> &str {
    let input = input.trim();
    for prefix in ["at ", "in ", "inside ", "under "] {
        if let Some(rest) = strip_ascii_case_prefix(input, prefix) {
            return rest.trim();
        }
    }
    input
}

fn split_create_directory_external_location(rest: &str) -> Option<(&str, &str)> {
    for delimiter in [" at ", " in ", " inside ", " under "] {
        if let Some((name, location)) = split_ascii_case_once(rest, delimiter) {
            if !name.trim().is_empty()
                && parse_external_directory_target(&directory_target_token(location)?).is_some()
            {
                return Some((name, location));
            }
        }
    }

    None
}

fn directory_target_token(input: &str) -> Option<String> {
    let input = trim_request_punctuation(input);
    let target = if let Some(rest) = input.strip_prefix('"') {
        let end = rest.find('"')?;
        &rest[..end]
    } else if let Some(rest) = input.strip_prefix('\'') {
        let end = rest.find('\'')?;
        &rest[..end]
    } else {
        input.split_whitespace().next()?
    };
    let target = trim_request_punctuation(target);
    (!target.is_empty()).then(|| target.to_string())
}

fn directory_name_target(input: &str) -> Option<String> {
    let (target, had_name_marker) = strip_directory_name_marker(input);
    let target = trim_location_filler_suffix(target);
    if !had_name_marker {
        return directory_target_token(target);
    }

    if target.starts_with('"') || target.starts_with('\'') {
        return directory_target_token(target);
    }

    let target = trim_request_punctuation(target);
    (!target.is_empty()).then(|| target.to_string())
}

fn trim_request_punctuation(input: &str) -> &str {
    input
        .trim()
        .trim_matches(|character: char| matches!(character, '.' | ',' | ';' | ':' | '?' | '!'))
}

fn parse_external_directory_target(target: &str) -> Option<PathBuf> {
    let target = trim_request_punctuation(target);
    if target.is_empty() {
        return None;
    }

    if is_desktop_path(target) {
        return desktop_path_from_target(target);
    }

    if target == "~" {
        return home_dir();
    }

    if let Some(rest) = target.strip_prefix("~/") {
        return home_dir().map(|home| home.join(rest));
    }

    if target.eq_ignore_ascii_case("$HOME") {
        return home_dir();
    }

    if let Some(rest) = strip_ascii_case_prefix(target, "$HOME/") {
        return home_dir().map(|home| home.join(rest));
    }

    let path = PathBuf::from(target);
    path.is_absolute().then_some(path)
}

fn desktop_directory_target(target: &str) -> Option<PathBuf> {
    let target = trim_request_punctuation(target);
    if target.is_empty() {
        return None;
    }

    Some(home_dir()?.join("Desktop").join(target))
}

fn desktop_path_from_target(target: &str) -> Option<PathBuf> {
    let target = trim_request_punctuation(target);
    if target.eq_ignore_ascii_case("desktop") {
        return home_dir().map(|home| home.join("Desktop"));
    }

    for prefix in ["desktop/", "~/desktop/", "$home/desktop/"] {
        if let Some(rest) = strip_ascii_case_prefix(target, prefix) {
            return home_dir().map(|home| home.join("Desktop").join(rest));
        }
    }

    None
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn shell_quote_path(path: &Path) -> String {
    let path = path.as_os_str().to_string_lossy();
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn shell_write_file_command(target_path: &Path, contents: &str) -> String {
    let delimiter = unique_heredoc_delimiter(contents);
    let mut body = contents.to_string();
    if !body.ends_with('\n') {
        body.push('\n');
    }
    let write_command = format!(
        "cat > {} <<'{}'\n{}{}",
        shell_quote_path(target_path),
        delimiter,
        body,
        delimiter
    );

    match target_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            format!("mkdir -p {} && {write_command}", shell_quote_path(parent))
        }
        _ => write_command,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectScaffoldPlan {
    directories: Vec<PathBuf>,
    files: Vec<(PathBuf, String)>,
}

fn build_project_scaffold_plan(base_path: &Path, plan_contents: &str) -> ProjectScaffoldPlan {
    if is_typescript_python_project_plan(plan_contents) {
        build_typescript_python_project_plan(base_path)
    } else if is_react_ts_project_plan(plan_contents) {
        build_react_ts_project_plan(base_path)
    } else {
        build_small_python_project_plan(base_path, plan_contents)
    }
}

fn is_typescript_python_project_plan(plan_contents: &str) -> bool {
    let normalized = plan_contents.to_ascii_lowercase();
    let mentions_typescript = mentions_typescript(&normalized)
        || normalized.contains(".ts")
        || normalized.contains("package.json")
        || normalized.contains("tsconfig");
    let mentions_python = normalized.contains("python")
        || normalized.contains(".py")
        || normalized.contains("requirements.txt");

    mentions_typescript && mentions_python
}

fn is_react_ts_project_plan(plan_contents: &str) -> bool {
    let normalized = plan_contents.to_ascii_lowercase();
    normalized.contains("react")
        && (mentions_typescript(&normalized)
            || normalized.contains("react ts")
            || normalized.contains("vite-style react scaffold")
            || normalized.contains("react project plan"))
}

fn build_small_python_project_plan(base_path: &Path, plan_contents: &str) -> ProjectScaffoldPlan {
    let title = plan_contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("# "))
        .unwrap_or("Small Python Project")
        .trim();
    let safe_title = if title.is_empty() {
        "Small Python Project"
    } else {
        title
    };

    let src_dir = base_path.join("src");
    let tests_dir = base_path.join("tests");
    let files = vec![
        (src_dir.join("__init__.py"), String::new()),
        (src_dir.join("csv_filter.py"), csv_filter_source()),
        (tests_dir.join("test_csv_filter.py"), csv_filter_test_source()),
        (
            base_path.join("README.md"),
            format!(
                "# {safe_title}\n\nA small Python project scaffold generated from the approved Markdown plan.\n\n## Run tests\n\n```bash\npython -m unittest discover -s tests\n```\n"
            ),
        ),
        (
            base_path.join("pyproject.toml"),
            "[project]\nname = \"elgar-small-python-project\"\nversion = \"0.1.0\"\nrequires-python = \">=3.10\"\n\n[tool.pytest.ini_options]\npythonpath = [\".\"]\n".to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir, tests_dir],
        files,
    }
}

fn build_typescript_python_project_plan(base_path: &Path) -> ProjectScaffoldPlan {
    let src_dir = base_path.join("src");
    let python_dir = base_path.join("python");
    let files = vec![
        (base_path.join("package.json"), ts_python_package_json()),
        (base_path.join("tsconfig.json"), ts_python_tsconfig_json()),
        (src_dir.join("main.ts"), ts_python_main_source()),
        (python_dir.join("main.py"), ts_python_python_main_source()),
        (base_path.join("requirements.txt"), "# Add Python dependencies here.\n".to_string()),
        (
            base_path.join("README.md"),
            "# TypeScript and Python Project\n\nA local scaffold generated from the verified Markdown plan.\n\n## TypeScript\n\n```bash\nnpm install\nnpm run build\n```\n\n## Python\n\n```bash\npython python/main.py\n```\n"
                .to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir, python_dir],
        files,
    }
}

fn build_react_ts_project_plan(base_path: &Path) -> ProjectScaffoldPlan {
    let src_dir = base_path.join("src");
    let files = vec![
        (base_path.join("package.json"), react_ts_package_json()),
        (base_path.join("index.html"), react_ts_index_html()),
        (base_path.join("tsconfig.json"), react_tsconfig_json()),
        (base_path.join("vite.config.ts"), react_ts_vite_config()),
        (src_dir.join("main.tsx"), react_ts_main_source()),
        (src_dir.join("App.tsx"), react_ts_app_source()),
        (src_dir.join("styles.css"), react_ts_styles_source()),
        (
            base_path.join("README.md"),
            "# React TS Project\n\nA local React TypeScript scaffold generated from the approved Markdown plan.\n\n## Deferred dependency install\n\nPackage installation is deferred. Propose and approve a separate shell command such as `npm install` before downloading dependencies.\n\n## After install\n\n```bash\nnpm run dev\n```\n"
                .to_string(),
        ),
    ];

    ProjectScaffoldPlan {
        directories: vec![src_dir],
        files,
    }
}

fn ts_python_package_json() -> String {
    r#"{
  "name": "elgar-typescript-python-project",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "build": "tsc --noEmit",
    "start": "node dist/main.js"
  },
  "devDependencies": {
    "typescript": "latest"
  }
}
"#
    .to_string()
}

fn ts_python_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "ESNext",
    "moduleResolution": "Node",
    "strict": true,
    "outDir": "dist",
    "rootDir": "src"
  },
  "include": ["src"]
}
"#
    .to_string()
}

fn ts_python_main_source() -> String {
    r#"export function greet(name: string): string {
  return `Hello, ${name}`;
}

console.log(greet("Elgar"));
"#
    .to_string()
}

fn ts_python_python_main_source() -> String {
    r#"from __future__ import annotations


def greet(name: str) -> str:
    return f"Hello, {name}"


if __name__ == "__main__":
    print(greet("Elgar"))
"#
    .to_string()
}

fn react_ts_package_json() -> String {
    r#"{
  "name": "elgar-react-ts-project",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview"
  },
  "dependencies": {
    "@vitejs/plugin-react": "latest",
    "vite": "latest",
    "typescript": "latest",
    "react": "latest",
    "react-dom": "latest"
  },
  "devDependencies": {}
}
"#
    .to_string()
}

fn react_ts_index_html() -> String {
    r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>React TS Project</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
"#
    .to_string()
}

fn react_tsconfig_json() -> String {
    r#"{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["DOM", "DOM.Iterable", "ES2020"],
    "allowJs": false,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "strict": true,
    "forceConsistentCasingInFileNames": true,
    "module": "ESNext",
    "moduleResolution": "Node",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx"
  },
  "include": ["src"],
  "references": []
}
"#
    .to_string()
}

fn react_ts_vite_config() -> String {
    r#"import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
});
"#
    .to_string()
}

fn react_ts_main_source() -> String {
    r#"import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
"#
    .to_string()
}

fn react_ts_app_source() -> String {
    r#"export default function App() {
  return (
    <main className="app-shell">
      <section>
        <p className="eyebrow">Elgar scaffold</p>
        <h1>React TS Project</h1>
        <p>
          This project was created from a controller-owned, approved plan.
          Install dependencies only after approving a separate shell command.
        </p>
      </section>
    </main>
  );
}
"#
    .to_string()
}

fn react_ts_styles_source() -> String {
    r#":root {
  color: #1f2937;
  background: #f8fafc;
  font-family:
    Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}

body {
  margin: 0;
}

.app-shell {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 32px;
}

section {
  width: min(680px, 100%);
}

.eyebrow {
  color: #0f766e;
  font-size: 0.8rem;
  font-weight: 700;
  letter-spacing: 0;
  text-transform: uppercase;
}

h1 {
  margin: 0 0 12px;
  font-size: 2.5rem;
}

p {
  line-height: 1.6;
}
"#
    .to_string()
}

fn csv_filter_source() -> String {
    r#"from __future__ import annotations

import argparse
import csv
from pathlib import Path


def filter_rows(input_path: Path, output_path: Path, column: str, value: str) -> int:
    with input_path.open(newline="") as source:
        reader = csv.DictReader(source)
        rows = [row for row in reader if row.get(column) == value]
        fieldnames = reader.fieldnames or []

    with output_path.open("w", newline="") as destination:
        writer = csv.DictWriter(destination, fieldnames=fieldnames, lineterminator="\n")
        writer.writeheader()
        writer.writerows(rows)

    return len(rows)


def main() -> None:
    parser = argparse.ArgumentParser(description="Filter a CSV by one column value.")
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("column")
    parser.add_argument("value")
    args = parser.parse_args()

    count = filter_rows(args.input, args.output, args.column, args.value)
    print(f"wrote {count} row(s)")


if __name__ == "__main__":
    main()
"#
    .to_string()
}

fn csv_filter_test_source() -> String {
    r#"from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from src.csv_filter import filter_rows


class CsvFilterTests(unittest.TestCase):
    def test_filters_rows_by_column_value(self) -> None:
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = root / "input.csv"
            output = root / "output.csv"
            source.write_text("name,kind\nalpha,keep\nbeta,drop\n", encoding="utf-8")

            count = filter_rows(source, output, "kind", "keep")

            self.assertEqual(count, 1)
            self.assertEqual(output.read_text(encoding="utf-8"), "name,kind\nalpha,keep\n")


if __name__ == "__main__":
    unittest.main()
"#
    .to_string()
}

fn shell_write_many_files_command(directories: &[PathBuf], files: &[(PathBuf, String)]) -> String {
    let mut lines = vec!["set -e".to_string()];
    let mut mkdir_paths = directories.to_vec();
    mkdir_paths.extend(
        files
            .iter()
            .filter_map(|(path, _contents)| path.parent().map(Path::to_path_buf)),
    );
    let mkdir_paths = dedupe_paths(mkdir_paths);
    if !mkdir_paths.is_empty() {
        lines.push(format!(
            "mkdir -p {}",
            mkdir_paths
                .iter()
                .map(|path| shell_quote_path(path))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }

    for (path, contents) in files {
        let delimiter = unique_heredoc_delimiter(contents);
        let mut body = contents.clone();
        if !body.ends_with('\n') {
            body.push('\n');
        }
        lines.push(format!(
            "cat > {} <<'{}'\n{}{}",
            shell_quote_path(path),
            delimiter,
            body,
            delimiter
        ));
    }

    lines.join("\n")
}

fn unique_heredoc_delimiter(contents: &str) -> String {
    let base = "ELGAR_MARKDOWN_PLAN_EOF";
    if !contents.lines().any(|line| line == base) {
        return base.to_string();
    }

    (1..)
        .map(|index| format!("{base}_{index}"))
        .find(|candidate| !contents.lines().any(|line| line == candidate))
        .expect("unbounded delimiter search should find a unique value")
}

fn display_path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn is_desktop_path(target: &str) -> bool {
    let normalized = target.trim().to_ascii_lowercase();
    normalized == "desktop"
        || normalized.starts_with("desktop/")
        || normalized.starts_with("~/desktop/")
        || normalized.starts_with("$home/desktop/")
}

fn parse_shell_command_request(input: &str) -> Option<String> {
    let trimmed = input.trim();
    for prefix in [
        "run shell command ",
        "run command ",
        "run shell ",
        "shell command ",
        "run ",
    ] {
        if let Some(command) = strip_ascii_case_prefix(trimmed, prefix) {
            let command = command.trim();
            if !command.is_empty() {
                return Some(command.to_string());
            }
        }
    }

    None
}

fn split_first_token(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim_start();
    let mut split = trimmed.splitn(2, char::is_whitespace);
    let target = split.next()?.trim();
    if target.is_empty() {
        return None;
    }
    Some((target, split.next().unwrap_or("")))
}

fn split_ascii_case_once<'a>(input: &'a str, delimiter: &str) -> Option<(&'a str, &'a str)> {
    let delimiter = delimiter.as_bytes();
    let index = input
        .as_bytes()
        .windows(delimiter.len())
        .position(|window| window.eq_ignore_ascii_case(delimiter))?;
    Some((&input[..index], &input[index + delimiter.len()..]))
}

fn strip_ascii_case_prefix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = input.get(..prefix.len())?;
    if candidate.eq_ignore_ascii_case(prefix) {
        Some(&input[prefix.len()..])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::{
        ffi::OsString,
        path::PathBuf,
        sync::{Arc, Mutex, MutexGuard},
    };

    use crate::{
        action::{
            Action, ActionLifecycleState, ActionRequest, FileActionVerification, ShellCommandAction,
        },
        event::{
            AssistantMessageSource, Event, ProviderMetrics, ProviderOutput, ProviderTokenUsage,
            VerifiedActionResult,
        },
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
        policy::PermissionPolicyMode,
        provider::{
            ChatToolDefinition, ControllerProvider, ProviderConfig, ProviderError,
            ProviderRequestMetadata, ProviderStub,
        },
        renderer::render_session,
        router::Route,
        session::{
            ActionRecord, Session, StructuredProjectPlan, StructuredProjectPlanStatus,
            VerifiedFolderReference, VerifiedPlanReference,
        },
    };

    use super::{Controller, VERIFIED_MEMORY_BYTE_LIMIT};

    fn session() -> Session {
        Session::new("session-1", ".", ".")
    }

    fn rooted_session(name: &str) -> (Session, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("elgar-controller-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        (Session::new("session-1", root.clone(), root.clone()), root)
    }

    static HOME_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        previous: Option<OsString>,
        _home_lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn set_home(value: &std::path::Path) -> Self {
            let home_lock = HOME_ENV_LOCK
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let previous = std::env::var_os("HOME");
            std::env::set_var("HOME", value);
            Self {
                previous,
                _home_lock: home_lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("HOME", previous);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn records_user_input_and_route_for_unknown_turn() {
        let controller = Controller::default();
        let mut session = session();

        let result = controller.turn(&mut session, "   ");

        assert_eq!(result.route, Route::Unknown);
        assert_eq!(session.events().len(), 2);
        assert!(matches!(session.events()[0], Event::UserMessage(_)));
        assert!(matches!(session.events()[1], Event::AssistantMessage(_)));
        assert!(session.actions().is_empty());
        assert_eq!(session.provider_metadata(), None);
    }

    #[test]
    fn ask_model_calls_provider_stub_and_records_provider_events() {
        let controller =
            Controller::new(ProviderStub::new("test-provider").with_model("stub-model"));
        let mut session = session();

        let result = controller.turn(&mut session, "what does this code do?");

        assert_eq!(result.route, Route::AskModel);
        assert_eq!(result.events.len(), 4);
        assert!(matches!(result.events[0], Event::UserMessage(_)));
        assert!(matches!(result.events[1], Event::ProviderStarted(_)));
        assert!(matches!(result.events[2], Event::ProviderFinished(_)));
        assert!(matches!(result.events[3], Event::AssistantMessage(_)));
        assert_eq!(
            session
                .provider_metadata()
                .as_ref()
                .map(|metadata| metadata.provider.as_str()),
            Some("test-provider")
        );
        assert!(session.actions().is_empty());
    }

    #[test]
    fn explicit_model_turn_sends_unclassified_chat_to_provider() {
        let controller = Controller::new(ProviderStub::new("test-provider"));
        let mut session = session();

        let result = controller.model_turn(&mut session, "sadsadad");

        assert_eq!(result.route, Route::AskModel);
        assert_eq!(result.events.len(), 4);
        assert!(matches!(result.events[0], Event::UserMessage(_)));
        assert!(matches!(result.events[1], Event::ProviderStarted(_)));
        assert!(matches!(result.events[2], Event::ProviderFinished(_)));
        assert!(matches!(result.events[3], Event::AssistantMessage(_)));
        assert!(session.actions().is_empty());
    }

    #[test]
    fn provider_text_is_recorded_as_provider_text_not_verified_truth() {
        let controller = Controller::default();
        let mut session = session();

        controller.turn(&mut session, "explain how to create hello.py");

        let provider_texts: Vec<&str> = session
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::ProviderFinished(finished) => Some(finished.output.text.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(provider_texts.len(), 1);
        assert!(provider_texts[0].contains("stub provider response"));
        assert!(session.actions().is_empty());
        assert!(session.actions().iter().all(|action| {
            !matches!(
                action.verified_result,
                Some(VerifiedActionResult::FileWritten { .. })
            )
        }));
    }

    #[test]
    fn ask_model_assistant_message_is_provider_sourced() {
        let controller = Controller::default();
        let mut session = session();

        controller.turn(&mut session, "what is rust?");

        let provider_message = session.events().iter().find_map(|event| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider =>
            {
                Some(message.content.as_str())
            }
            _ => None,
        });

        assert!(provider_message.is_some_and(|message| message.contains("stub provider response")));
    }

    #[test]
    fn greeting_routes_to_stub_chat_with_no_network_guidance() {
        let controller = Controller::default();
        let mut session = session();

        let result = controller.turn(&mut session, "hello!");

        assert_eq!(result.route, Route::AskModel);
        assert!(session.actions().is_empty());
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(started) if started.provider == "stub-provider")));

        let provider_message = session.events().iter().find_map(|event| match event {
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider =>
            {
                Some(message.content.as_str())
            }
            _ => None,
        });

        assert!(provider_message.is_some_and(|message| {
            message.contains("stub provider response (no-network) to: hello!")
                && message.contains("No live provider call was made")
                && message.contains("tui-controller-smoke")
        }));
    }

    #[test]
    fn non_provider_routes_do_not_call_provider() {
        let controller = Controller::default();
        let mut session = session();

        for input in ["help", "approve", "reject", "create hello.py"] {
            let result = controller.turn(&mut session, input);
            assert_ne!(result.route, Route::AskModel);
        }

        assert!(session.events().iter().all(|event| !matches!(
            event,
            Event::ProviderStarted(_) | Event::ProviderFinished(_)
        )));
        assert_eq!(session.provider_metadata(), None);
    }

    #[test]
    fn provider_stub_turn_does_not_create_files() {
        let controller = Controller::default();
        let mut session = session();
        let path = std::env::temp_dir().join(format!(
            "elgar-provider-stub-{}-hello.py",
            std::process::id()
        ));

        assert!(!path.exists());

        controller.turn(&mut session, "explain how to write hello.py");

        assert!(!path.exists());
        assert!(session.actions().is_empty());
    }

    #[test]
    fn proposed_write_file_turn_records_action_without_creating_file() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("proposed");
        let path = root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        let result = controller.turn(&mut session, "create hello.py");

        assert_eq!(result.route, Route::ProposeWriteFile);
        assert!(!path.exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
    }

    #[test]
    fn rejected_write_file_turn_does_not_create_file_and_is_terminal() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("rejected");
        let path = root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        controller.turn(&mut session, "reject");
        controller.turn(&mut session, "approve");

        assert!(!path.exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Rejected
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionRejected(_))));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApplied(_))));
    }

    #[test]
    fn approved_write_file_turn_writes_target_and_records_verified_result() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("approved");
        let path = root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        controller.turn(&mut session, "approve");

        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert_eq!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::FileWritten {
                path: path.display().to_string()
            })
        );
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApproved(_))));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_absolute_write_file_turn_fails_without_writing() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("absolute");
        let path = std::env::temp_dir().join(format!(
            "elgar-controller-{}-absolute.py",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, &format!("create {}", path.display()));
        controller.turn(&mut session, "approve");

        assert!(!path.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("absolute paths are not allowed")));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_parent_traversal_write_file_turn_fails_without_writing() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("traversal");
        let outside = root.parent().unwrap().join(format!(
            "elgar-controller-{}-outside.py",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&outside);

        controller.turn(
            &mut session,
            &format!(
                "create ../{}",
                outside.file_name().unwrap().to_string_lossy()
            ),
        );
        controller.turn(&mut session, "approve");

        assert!(!outside.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0]
            .failure_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("parent directory traversal is not allowed")));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn failed_write_file_records_failure_without_verified_result() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("missing-parent");
        let path = root.join("missing").join("hello.py");

        controller.turn(&mut session, "create missing/hello.py");
        controller.turn(&mut session, "approve");

        assert!(!path.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0].failure_reason.is_some());
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
    }

    #[test]
    fn proposed_patch_file_turn_records_action_without_changing_file() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("proposed-patch");
        let path = root.join("notes.txt");
        std::fs::write(&path, "old contents").unwrap();

        let result = controller.turn(&mut session, "edit file notes.txt replace old with new");

        assert_eq!(result.route, Route::ProposePatchFile);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old contents");
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_patch_file_turn_updates_target_and_records_verified_result() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("approved-patch");
        let path = root.join("notes.txt");
        std::fs::write(&path, "old contents").unwrap();

        controller.turn(&mut session, "edit file notes.txt replace old with new");
        controller.turn(&mut session, "approve");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new contents");
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert_eq!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::File(
                FileActionVerification::FilePatched {
                    path: path.display().to_string()
                }
            ))
        );
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn rejected_overwrite_file_turn_does_not_change_file() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("rejected-overwrite");
        let path = root.join("notes.txt");
        std::fs::write(&path, "original").unwrap();

        controller.turn(&mut session, "overwrite file notes.txt with replacement");
        controller.turn(&mut session, "reject");
        controller.turn(&mut session, "approve");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Rejected
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn approved_overwrite_file_turn_replaces_target_and_records_verified_result() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("approved-overwrite");
        let path = root.join("notes.txt");
        std::fs::write(&path, "original").unwrap();

        let proposed = controller.turn(&mut session, "overwrite file notes.txt with replacement");
        controller.turn(&mut session, "approve");

        assert_eq!(proposed.route, Route::ProposeOverwriteFile);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert_eq!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::File(
                FileActionVerification::FileOverwritten {
                    path: path.display().to_string()
                }
            ))
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn provider_text_cannot_apply_existing_action_or_create_file() {
        let controller = Controller::default();
        let (mut session, _root) = rooted_session("provider");
        let path = session.project_root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        controller.turn(&mut session, "explain how to write the file");

        assert!(!path.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
    }

    #[derive(Debug, Clone)]
    struct FakeProvider {
        output: Result<ProviderOutput, ProviderError>,
    }

    impl FakeProvider {
        fn success(text: impl Into<String>) -> Self {
            Self {
                output: Ok(ProviderOutput::new(text)),
            }
        }

        fn output(output: ProviderOutput) -> Self {
            Self { output: Ok(output) }
        }

        fn failure(message: impl Into<String>) -> Self {
            Self {
                output: Err(ProviderError::provider(message, Some(404), None)),
            }
        }
    }

    impl ControllerProvider for FakeProvider {
        fn request_metadata(&self) -> crate::provider::ProviderRequestMetadata {
            crate::provider::ProviderRequestMetadata::new(
                "fake-provider",
                Some("fake-model".to_string()),
                "fake-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            self.output.clone()
        }
    }

    #[derive(Debug, Clone)]
    struct CapturingProvider {
        prompts: Arc<Mutex<Vec<String>>>,
    }

    impl CapturingProvider {
        fn new(prompts: Arc<Mutex<Vec<String>>>) -> Self {
            Self { prompts }
        }
    }

    impl ControllerProvider for CapturingProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "capture-provider",
                Some("capture-model".to_string()),
                "capture-request-1",
            )
        }

        fn chat(&self, prompt: &str) -> Result<ProviderOutput, ProviderError> {
            self.prompts.lock().unwrap().push(prompt.to_string());
            Ok(ProviderOutput::new("captured"))
        }
    }

    #[derive(Debug, Clone)]
    struct ToolEnabledFakeProvider {
        output: Result<ProviderOutput, ProviderError>,
        received_tool_names: Arc<Mutex<Vec<Vec<String>>>>,
        chat_call_count: Arc<Mutex<usize>>,
    }

    impl ToolEnabledFakeProvider {
        fn new(output: ProviderOutput) -> (Self, Arc<Mutex<Vec<Vec<String>>>>, Arc<Mutex<usize>>) {
            let received_tool_names = Arc::new(Mutex::new(Vec::new()));
            let chat_call_count = Arc::new(Mutex::new(0));
            (
                Self {
                    output: Ok(output),
                    received_tool_names: Arc::clone(&received_tool_names),
                    chat_call_count: Arc::clone(&chat_call_count),
                },
                received_tool_names,
                chat_call_count,
            )
        }
    }

    impl ControllerProvider for ToolEnabledFakeProvider {
        fn request_metadata(&self) -> ProviderRequestMetadata {
            ProviderRequestMetadata::new(
                "tool-provider",
                Some("tool-model".to_string()),
                "tool-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            *self.chat_call_count.lock().unwrap() += 1;
            Ok(ProviderOutput::new("legacy chat path"))
        }

        fn chat_with_tools_with_metadata(
            &self,
            _prompt: &str,
            _metadata: &ProviderRequestMetadata,
            tools: Vec<ChatToolDefinition>,
        ) -> Result<ProviderOutput, ProviderError> {
            self.received_tool_names.lock().unwrap().push(
                tools
                    .iter()
                    .map(|tool| tool.function.name.clone())
                    .collect(),
            );
            self.output.clone()
        }
    }

    fn raw_model_tool_call(
        id: &str,
        name: RawModelToolName,
        arguments: serde_json::Value,
    ) -> RawModelToolCall {
        RawModelToolCall {
            id: id.to_string(),
            name,
            arguments,
            assistant_summary: None,
        }
    }

    fn seed_verified_folder(session: &mut Session, root: &std::path::Path, name: &str) -> PathBuf {
        let project_root = root.join(name);
        std::fs::create_dir_all(&project_root).unwrap();
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: project_root.clone(),
            source_action_id: format!("action-folder-{name}"),
        });
        project_root
    }

    fn seed_verified_react_ts_plan(
        session: &mut Session,
        root: &std::path::Path,
        name: &str,
    ) -> (PathBuf, PathBuf) {
        let project_root = seed_verified_folder(session, root, name);
        let plan_path = project_root.join("react-ts-project-plan.md");
        std::fs::write(
            &plan_path,
            format!(
                "# React TS Project Plan\n\nProject root: {}\n\n- Add a Vite-style React scaffold.\n- Defer package installation.\n",
                project_root.display()
            ),
        )
        .unwrap();
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project_root.clone(),
            source_action_id: format!("action-plan-{name}"),
        });
        (project_root, plan_path)
    }

    #[derive(Debug, Clone)]
    struct StreamingFakeProvider;

    impl ControllerProvider for StreamingFakeProvider {
        fn request_metadata(&self) -> crate::provider::ProviderRequestMetadata {
            crate::provider::ProviderRequestMetadata::new(
                "stream-provider",
                Some("stream-model".to_string()),
                "stream-request-1",
            )
        }

        fn chat(&self, _prompt: &str) -> Result<ProviderOutput, ProviderError> {
            Ok(ProviderOutput::new("I approved and wrote hello.py."))
        }

        fn chat_stream(
            &self,
            _prompt: &str,
            on_chunk: &mut dyn FnMut(crate::provider::ProviderStreamChunk),
        ) -> Result<ProviderOutput, ProviderError> {
            on_chunk(crate::provider::ProviderStreamChunk::Reasoning(
                "Need to describe only.".to_string(),
            ));
            on_chunk(crate::provider::ProviderStreamChunk::Text(
                "I approved and wrote hello.py.".to_string(),
            ));
            Ok(ProviderOutput::new("I approved and wrote hello.py.")
                .with_thinking("Need to describe only."))
        }
    }

    #[test]
    fn model_first_text_only_provider_output_records_text_and_no_action() {
        let (provider, received_tools, chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("I can help with that."));
        let controller = Controller::new(provider);
        let mut session = session();

        let result = controller.model_first_turn_with_policy(
            &mut session,
            "explain the project",
            PermissionPolicyMode::ReviewAll,
        );

        assert_eq!(result.route, Route::AskModel);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(received_tools.lock().unwrap()[0].len(), 8);
        assert!(received_tools.lock().unwrap()[0].contains(&"ask_guidance".to_string()));
        assert!(received_tools.lock().unwrap()[0].contains(&"create_file".to_string()));
        assert!(session.actions().is_empty());
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ProviderFinished(_))));
        assert!(result.events.iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Provider
                        && message.content == "I can help with that."
            )
        }));
    }

    #[test]
    fn model_first_unrelated_text_still_uses_provider_tool_chat() {
        let (provider, received_tools, chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("provider answered"));
        let controller = Controller::new(provider);
        let mut session = session();

        controller.model_first_turn_with_policy(
            &mut session,
            "tell me about Rust ownership",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Provider
                        && message.content == "provider answered"
            )
        }));
    }

    #[test]
    fn model_first_create_file_auto_create_policy_writes_and_verifies_without_approve() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-file",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "model-first.txt", "contents": "created by policy" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-create-file");

        let result = controller.model_first_turn_with_policy(
            &mut session,
            "create model-first.txt",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let path = root.join("model-first.txt");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "created by policy");
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(matches!(
            &session.actions()[0].action.request,
            ActionRequest::CreateFile(_)
        ));
        assert_eq!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::FileWritten {
                path: path.display().to_string()
            })
        );
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.is_policy_approved()));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApproved(_))));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionApplied(_))));
        assert!(result
            .events
            .iter()
            .all(|event| !matches!(event, Event::ActionProposed(_))));
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_create_directory_auto_create_policy_creates_and_verifies_without_approve() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "model-first-dir" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-create-dir");

        controller.model_first_turn_with_policy(
            &mut session,
            "create model-first-dir",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let path = root.join("model-first-dir");
        assert!(path.is_dir());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(matches!(
            &session.actions()[0].action.request,
            ActionRequest::CreateDirectory(_)
        ));
        assert_eq!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::File(
                FileActionVerification::DirectoryCreated {
                    path: path.display().to_string()
                }
            ))
        );
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.is_policy_approved()));
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_capability_question_create_directory_tool_call_does_not_mutate() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "new_folder" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-capability-question-create");

        controller.model_first_turn_with_policy(
            &mut session,
            "can you create a folder for me?",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!root.join("new_folder").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
            )
        }));
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content.contains("imperative request")
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_guidance_tool_only_asks_question_and_creates_no_action() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-guidance",
            RawModelToolName::Known(ModelToolName::AskGuidance),
            json!({
                "question": "Which folder should I use?",
                "reason": "No verified folder is available."
            }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-guidance-tool-only");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a project in that folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
            )
        }));
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content == "Which folder should I use?"
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_guidance_plus_action_blocks_mutation_and_asks_question() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-guidance",
                RawModelToolName::Known(ModelToolName::AskGuidance),
                json!({ "question": "Which folder should I use?" }),
            ),
            raw_model_tool_call(
                "call-create-dir",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "guessed-folder" }),
            ),
        ]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-guidance-plus-action");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a project in that folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!root.join("guessed-folder").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content == "Which folder should I use?"
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_uncertainty_text_with_action_blocks_mutation() {
        let output =
            ProviderOutput::new("I'm not sure which folder you mean, but I will create this.")
                .with_tool_calls(vec![raw_model_tool_call(
                    "call-create-dir",
                    RawModelToolName::Known(ModelToolName::CreateDirectory),
                    json!({ "target_path": "uncertain-folder" }),
                )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-uncertain-action");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a project in that folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!root.join("uncertain-folder").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
            )
        }));
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content.contains("clarification")
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_stub_ambiguous_that_folder_request_asks_guidance_without_create() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("model-first-that-folder-guidance");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a project in that folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(session.actions().is_empty());
        assert!(!root.join("project").exists());
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content == "Which folder should I use for the project?"
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_imperative_create_directory_still_auto_creates() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "imperative_folder" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-imperative-create-dir");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a folder called imperative_folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(root.join("imperative_folder").is_dir());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_truth_guard_replaces_false_folder_denial_after_verified_create() {
        let create_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "truth_folder" }),
        )]);
        let (create_provider, _received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(create_output);
        let create_controller = Controller::new(create_provider);
        let (mut session, root) = rooted_session("model-first-truth-guard-folder");

        create_controller.model_first_turn_with_policy(
            &mut session,
            "create a folder called truth_folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );
        assert!(root.join("truth_folder").is_dir());

        let (deny_provider, _received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("No folder was created."));
        let deny_controller = Controller::new(deny_provider);
        deny_controller.model_first_turn_with_policy(
            &mut session,
            "i was just asking! i didnt tell you to do it!",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let visible_messages = session
            .events()
            .iter()
            .filter_map(|event| match event {
                Event::AssistantMessage(message) => Some(message.content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!visible_messages
            .to_ascii_lowercase()
            .contains("no folder was created"));
        assert!(visible_messages.contains("Filesystem truth:"));
        assert!(!render_session(&session)
            .to_ascii_lowercase()
            .contains("no folder was created"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_multiple_safe_create_tool_calls_auto_apply_and_verify_all() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-create-dir",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app" }),
            ),
            raw_model_tool_call(
                "call-create-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app/src" }),
            ),
            raw_model_tool_call(
                "call-create-index",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/index.html", "contents": "<div id=\"root\"></div>\n" }),
            ),
            raw_model_tool_call(
                "call-create-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-safe-create-batch");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a static React starter",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(root.join("app").is_dir());
        assert!(root.join("app/src").is_dir());
        assert_eq!(
            std::fs::read_to_string(root.join("app/index.html")).unwrap(),
            "<div id=\"root\"></div>\n"
        );
        assert!(root.join("app/src/App.tsx").is_file());
        assert_eq!(session.actions().len(), 4);
        assert!(session
            .actions()
            .iter()
            .all(|record| record.action.state == ActionLifecycleState::Applied));
        assert!(session
            .actions()
            .iter()
            .all(|record| record.verified_result.is_some()));
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_create_file_review_all_still_proposes_only() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-file",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "review-all.txt", "contents": "draft only" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-review-all-create-file");

        let result = controller.model_first_turn_with_policy(
            &mut session,
            "create review-all.txt",
            PermissionPolicyMode::ReviewAll,
        );

        assert!(!root.join("review-all.txt").exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.user_approval_required));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionProposed(_))));
        assert!(result
            .events
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_auto_create_existing_file_does_not_overwrite_or_succeed_silently() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-file",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "existing.txt", "contents": "new contents" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-existing-create-file");
        let path = root.join("existing.txt");
        std::fs::write(&path, "original contents").unwrap();

        let result = controller.model_first_turn_with_policy(
            &mut session,
            "create existing.txt",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original contents");
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0].failure_reason.is_some());
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.is_policy_approved()));
        assert!(result
            .events
            .iter()
            .any(|event| matches!(event, Event::ActionFailed(_))));
        assert!(result
            .events
            .iter()
            .all(|event| !matches!(event, Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_batch_existing_file_conflict_records_partial_truth_without_overwrite() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-existing",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "existing.txt", "contents": "new contents" }),
            ),
            raw_model_tool_call(
                "call-new",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "new.txt", "contents": "new file" }),
            ),
        ]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-existing-batch");
        let existing = root.join("existing.txt");
        std::fs::write(&existing, "original contents").unwrap();

        controller.model_first_turn_with_policy(
            &mut session,
            "create the files",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(
            std::fs::read_to_string(&existing).unwrap(),
            "original contents"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("new.txt")).unwrap(),
            "new file"
        );
        assert_eq!(session.actions().len(), 2);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );
        assert!(session.actions()[0].failure_reason.is_some());
        assert_eq!(
            session.actions()[1].action.state,
            ActionLifecycleState::Applied
        );
        assert!(session.actions()[1].verified_result.is_some());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_shell_command_stays_review_gated_in_auto_create_policy() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-shell",
            RawModelToolName::Known(ModelToolName::ShellCommand),
            json!({ "command": "touch shell-created.txt", "cwd": "." }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-shell");

        controller.model_first_turn_with_policy(
            &mut session,
            "run touch shell-created.txt",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!root.join("shell-created.txt").exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert!(matches!(
            &session.actions()[0].action.request,
            ActionRequest::ShellCommand(_)
        ));
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session.actions()[0]
            .policy_decision
            .as_ref()
            .is_some_and(|decision| decision.user_approval_required));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_edit_delete_and_move_stay_review_gated_in_auto_create_policy() {
        let cases = [
            (
                ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                    "call-overwrite",
                    RawModelToolName::Known(ModelToolName::OverwriteFile),
                    json!({ "target_path": "existing.txt", "contents": "replacement" }),
                )]),
                "overwrite existing.txt",
            ),
            (
                ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                    "call-patch",
                    RawModelToolName::Known(ModelToolName::PatchFile),
                    json!({ "target_path": "existing.txt", "find": "original", "replace": "patched" }),
                )]),
                "patch existing.txt",
            ),
            (
                ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                    "call-delete",
                    RawModelToolName::Known(ModelToolName::DeleteFile),
                    json!({ "target_path": "existing.txt" }),
                )]),
                "delete existing.txt",
            ),
            (
                ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                    "call-move",
                    RawModelToolName::Known(ModelToolName::MoveFile),
                    json!({ "source_path": "existing.txt", "target_path": "moved.txt" }),
                )]),
                "move existing.txt",
            ),
        ];

        for (index, (output, input)) in cases.into_iter().enumerate() {
            let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
            let controller = Controller::new(provider);
            let (mut session, root) =
                rooted_session(&format!("model-first-auto-create-review-gated-{index}"));
            std::fs::write(root.join("existing.txt"), "original").unwrap();

            controller.model_first_turn_with_policy(
                &mut session,
                input,
                PermissionPolicyMode::AutoCreateReviewModify,
            );

            assert_eq!(
                std::fs::read_to_string(root.join("existing.txt")).unwrap(),
                "original"
            );
            assert!(!root.join("moved.txt").exists());
            assert_eq!(session.actions().len(), 1);
            assert_eq!(
                session.actions()[0].action.state,
                ActionLifecycleState::Proposed
            );
            assert_eq!(session.actions()[0].verified_result, None);
            assert!(session.actions()[0]
                .policy_decision
                .as_ref()
                .is_some_and(|decision| decision.user_approval_required));
            assert!(session
                .events()
                .iter()
                .any(|event| { matches!(event, Event::ActionProposed(_)) }));
            assert!(session.events().iter().all(|event| {
                !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))
            }));

            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn model_first_mixed_batch_does_not_auto_apply_unsafe_actions() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-create-dir",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "app" }),
            ),
            raw_model_tool_call(
                "call-create-file",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "app/index.html", "contents": "<div></div>\n" }),
            ),
            raw_model_tool_call(
                "call-shell",
                RawModelToolName::Known(ModelToolName::ShellCommand),
                json!({ "command": "touch shell-created.txt", "cwd": "." }),
            ),
            raw_model_tool_call(
                "call-overwrite",
                RawModelToolName::Known(ModelToolName::OverwriteFile),
                json!({ "target_path": "existing.txt", "contents": "replacement" }),
            ),
            raw_model_tool_call(
                "call-delete",
                RawModelToolName::Known(ModelToolName::DeleteFile),
                json!({ "target_path": "existing.txt" }),
            ),
            raw_model_tool_call(
                "call-move",
                RawModelToolName::Known(ModelToolName::MoveFile),
                json!({ "source_path": "existing.txt", "target_path": "moved.txt" }),
            ),
        ]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-mixed-batch");
        std::fs::write(root.join("existing.txt"), "original").unwrap();

        controller.model_first_turn_with_policy(
            &mut session,
            "create app files and run setup",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(root.join("app").is_dir());
        assert!(root.join("app/index.html").is_file());
        assert_eq!(
            std::fs::read_to_string(root.join("existing.txt")).unwrap(),
            "original"
        );
        assert!(!root.join("shell-created.txt").exists());
        assert!(!root.join("moved.txt").exists());
        assert!(session.actions().iter().any(|record| {
            matches!(
                record.action.request,
                ActionRequest::CreateDirectory(_) | ActionRequest::CreateFile(_)
            ) && record.action.state == ActionLifecycleState::Applied
        }));
        assert!(session.actions().iter().any(|record| {
            matches!(record.action.request, ActionRequest::ShellCommand(_))
                && record.action.state == ActionLifecycleState::Proposed
        }));
        assert!(session.actions().iter().all(|record| {
            !matches!(
                record.action.request,
                ActionRequest::OverwriteFile(_)
                    | ActionRequest::DeleteFile(_)
                    | ActionRequest::MoveFile(_)
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_unknown_and_malformed_tool_calls_fail_safely() {
        let cases = [
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-unknown",
                RawModelToolName::Unknown("unknown_tool".to_string()),
                json!({}),
            )]),
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-malformed",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "contents": "missing target" }),
            )]),
        ];

        for output in cases {
            let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
            let controller = Controller::new(provider);
            let mut session = session();

            controller.model_first_turn_with_policy(
                &mut session,
                "draft a tool call",
                PermissionPolicyMode::ReviewAll,
            );

            assert!(session.actions().is_empty());
            assert!(session
                .events()
                .iter()
                .any(|event| matches!(event, Event::Error(_))));
            assert!(session.events().iter().all(|event| {
                !matches!(
                    event,
                    Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
                )
            }));
        }
    }

    #[test]
    fn model_first_provider_prose_claiming_success_without_tool_call_creates_no_truth() {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
            ProviderOutput::new("Done, I created success.txt and verified it."),
        );
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-prose-only");

        controller.model_first_turn_with_policy(
            &mut session,
            "create success.txt",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!root.join("success.txt").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )));
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content.contains("did not receive a tool call")
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_prose_only_implement_plan_does_not_fake_success() {
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
            ProviderOutput::new("Done, I implemented the plan and created package.json."),
        );
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-prose-implement-plan");
        let project_root = root.join("planned-app");
        std::fs::create_dir_all(&project_root).unwrap();
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: project_root.clone(),
            source_action_id: "action-folder".to_string(),
        });

        controller.model_first_turn_with_policy(
            &mut session,
            "implement the plan",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(!project_root.join("package.json").exists());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| !matches!(
            event,
            Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
        )));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_simple_desktop_folder_can_apply_model_tool_path() {
        let (mut session, root) = rooted_session("model-first-desktop-folder");
        let home = root.join("home");
        let desktop = home.join("Desktop");
        let desktop_target = desktop.join("ElgarRetest-267");
        std::fs::create_dir_all(&desktop).unwrap();
        let _home = EnvGuard::set_home(&home);
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": desktop_target.display().to_string() }),
        )]);
        let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);

        controller.model_first_turn_with_policy(
            &mut session,
            "create a folder called ElgarRetest-267 in the desktop",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(desktop_target.is_dir());
        assert!(!root.join("ElgarRetest-267").exists());
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(matches!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::File(
                FileActionVerification::DirectoryCreated { .. }
            ))
        ));
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));
        assert_eq!(
            session
                .project_memory()
                .latest_verified_folder()
                .map(|reference| reference.path.as_path()),
            Some(desktop_target.as_path())
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_compound_folder_project_request_applies_model_tool_calls() {
        let (mut session, root) = rooted_session("model-first-compound-folder-project-tools");
        let home = root.join("home");
        let desktop = home.join("Desktop");
        let project_root = desktop.join("Demo123");
        std::fs::create_dir_all(&desktop).unwrap();
        let _home = EnvGuard::set_home(&home);
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-create-root",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": project_root.display().to_string() }),
            ),
            raw_model_tool_call(
                "call-create-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({
                    "target_path": project_root.join("calculator.py").display().to_string(),
                    "contents": "print('calculator UI placeholder')\n"
                }),
            ),
            raw_model_tool_call(
                "call-create-readme",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({
                    "target_path": project_root.join("README.md").display().to_string(),
                    "contents": "# Demo123 Calculator\n"
                }),
            ),
        ]);
        let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);

        controller.model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI.",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let greedy_clause = "inside the folder create a python project of a calculator with UI";
        let greedy_target = desktop.join(format!("Demo123, {greedy_clause}"));
        assert!(project_root.is_dir());
        assert!(project_root.join("calculator.py").is_file());
        assert!(project_root.join("README.md").is_file());
        assert!(!greedy_target.exists());
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert_eq!(session.actions().len(), 3);
        assert!(session
            .actions()
            .iter()
            .all(|record| record.action.state == ActionLifecycleState::Applied));
        for record in session.actions() {
            assert!(!record.action.summary.contains(greedy_clause));
            assert!(!record
                .action
                .request
                .approval_target()
                .contains(greedy_clause));
            assert!(record.verified_result.is_some());
        }
        assert_eq!(
            session
                .project_memory()
                .latest_verified_folder()
                .map(|reference| reference.path.as_path()),
            Some(project_root.as_path())
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_compound_folder_project_prose_only_creates_no_malformed_folder() {
        let (provider, received_tools, chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("I need tool calls to create files."));
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-compound-folder-project-prose");
        let home = root.join("home");
        let desktop = home.join("Desktop");
        std::fs::create_dir_all(&desktop).unwrap();
        let _home = EnvGuard::set_home(&home);

        controller.model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI.",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let greedy_target = desktop
            .join("Demo123, inside the folder create a python project of a calculator with UI");
        assert!(!desktop.join("Demo123").exists());
        assert!(!greedy_target.exists());
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert!(session.actions().is_empty());
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::ActionProposed(_) | Event::ActionApproved(_) | Event::ActionApplied(_)
            )
        }));
        assert!(session.events().iter().all(|event| {
            !matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Controller
                        && message.content.contains("Created")
            )
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_provider_stub_compound_folder_project_emits_project_tool_calls() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("model-first-stub-compound-project-tools");
        let home = root.join("home");
        let desktop = home.join("Desktop");
        let project_root = desktop.join("Demo123");
        std::fs::create_dir_all(&desktop).unwrap();
        let _home = EnvGuard::set_home(&home);

        controller.model_first_turn_with_policy(
            &mut session,
            "create a folder on the desktop and name it Demo123, inside the folder create a python project of a calculator with UI",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(project_root.is_dir());
        assert!(project_root.join("calculator.py").is_file());
        assert!(project_root.join("README.md").is_file());
        assert!(!desktop
            .join("Demo123, inside the folder create a python project of a calculator with UI")
            .exists());
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(_))));
        assert!(session
            .actions()
            .iter()
            .all(|record| record.action.state == ActionLifecycleState::Applied));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_plan_request_uses_provider_tool_call_and_verified_folder_context() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-plan",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({
                "target_path": "react-ts-project-plan.md",
                "contents": "# React TS Project Plan\n\n- Use model tools.\n"
            }),
        )]);
        let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-plan-same-folder");
        let project_root = seed_verified_folder(&mut session, &root, "verified-react");

        controller.model_first_turn_with_policy(
            &mut session,
            "create a plan for a simple React TS project in the same folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let plan_path = project_root.join("react-ts-project-plan.md");
        assert!(plan_path.is_file());
        assert!(!root.join("react-ts-project-plan.md").exists());
        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(_))));
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        let ActionRequest::CreateFile(action) = &session.actions()[0].action.request else {
            panic!("expected CreateFile plan action");
        };
        assert_eq!(
            action.target_path,
            PathBuf::from("verified-react/react-ts-project-plan.md")
        );
        assert!(session.actions()[0].verified_result.is_some());
        assert_eq!(
            session
                .project_memory()
                .latest_verified_plan()
                .map(|reference| reference.path.as_path()),
            Some(plan_path.as_path())
        );
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_implement_plan_applies_provider_tool_calls_in_verified_plan_root() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-package",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "package.json", "contents": "{\"scripts\":{}}\n" }),
            ),
            raw_model_tool_call(
                "call-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
        let (provider, received_tools, chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-implement-verified-plan");
        let (project_root, _plan_path) =
            seed_verified_react_ts_plan(&mut session, &root, "verified-react");

        controller.model_first_turn_with_policy(
            &mut session,
            "implement the plan",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(*chat_calls.lock().unwrap(), 0);
        assert_eq!(session.actions().len(), 3);
        assert!(session
            .actions()
            .iter()
            .all(|record| record.action.state == ActionLifecycleState::Applied));
        assert!(session
            .actions()
            .iter()
            .all(|record| record.verified_result.is_some()));
        assert!(project_root.join("package.json").is_file());
        assert!(project_root.join("src").is_dir());
        assert!(project_root.join("src/App.tsx").is_file());
        assert!(!root.join("package.json").exists());
        assert!(session.project_memory().latest_structured_plan().is_none());
        assert!(matches!(
            session.pending_action_selection(),
            crate::session::PendingActionSelection::None
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_create_rest_of_project_uses_provider_tools_and_verified_plan_path() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-main",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/main.tsx", "contents": "void 0;\n" }),
            ),
        ]);
        let (provider, received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-rest-of-project");
        let (project_root, _plan_path) =
            seed_verified_react_ts_plan(&mut session, &root, "verified-rest");

        controller.model_first_turn_with_policy(
            &mut session,
            "create the rest of the project",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(session.actions().len(), 2);
        assert!(project_root.join("src/main.tsx").is_file());
        assert!(!root.join("src/main.tsx").exists());
        assert!(session
            .actions()
            .iter()
            .all(|record| record.action.state == ActionLifecycleState::Applied));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_create_project_you_planned_uses_provider_tools_only() {
        let (mut session, root) = rooted_session("model-first-create-project-you-planned");

        let create_folder_output =
            ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
                "call-create-folder",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "planned-hybrid" }),
            )]);
        let (create_folder_provider, _received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(create_folder_output);
        let create_folder_controller = Controller::new(create_folder_provider);
        create_folder_controller.model_first_turn_with_policy(
            &mut session,
            "create a folder called planned-hybrid",
            PermissionPolicyMode::AutoCreateReviewModify,
        );
        let project_root = root.join("planned-hybrid");
        assert!(project_root.is_dir());

        let plan_contents = format!(
            "# TS and Python Project Plan\n\nProject root: {}\n\n- Add TypeScript files: `package.json`, `tsconfig.json`, and `src/main.ts`.\n- Add Python files: `python/main.py` and `requirements.txt`.\n- Add a README with run instructions.\n",
            project_root.display()
        );
        let write_plan_output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-plan",
            RawModelToolName::Known(ModelToolName::CreateFile),
            json!({ "target_path": "project-plan.md", "contents": plan_contents }),
        )]);
        let (write_plan_provider, _received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(write_plan_output);
        let write_plan_controller = Controller::new(write_plan_provider);
        write_plan_controller.model_first_turn_with_policy(
            &mut session,
            "write a TypeScript and Python project plan inside that folder",
            PermissionPolicyMode::AutoCreateReviewModify,
        );
        assert!(project_root.join("project-plan.md").is_file());

        let (read_plan_provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(
            ProviderOutput::new("The verified plan describes TypeScript and Python project files."),
        );
        let read_plan_controller = Controller::new(read_plan_provider);
        read_plan_controller.model_first_turn_with_policy(
            &mut session,
            "read the plan you wrote",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        let actions_before_execute = session.actions().len();
        let (prose_provider, prose_received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("Done, I created the project."));
        let prose_controller = Controller::new(prose_provider);
        prose_controller.model_first_turn_with_policy(
            &mut session,
            "create the project you planned",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(prose_received_tools.lock().unwrap().len(), 1);
        assert_eq!(session.actions().len(), actions_before_execute);
        assert!(!project_root.join("package.json").exists());

        let execute_output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-python",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "python" }),
            ),
            raw_model_tool_call(
                "call-package",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "package.json", "contents": "{}\n" }),
            ),
            raw_model_tool_call(
                "call-tsconfig",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "tsconfig.json", "contents": "{}\n" }),
            ),
            raw_model_tool_call(
                "call-main-ts",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/main.ts", "contents": "console.log('ok');\n" }),
            ),
            raw_model_tool_call(
                "call-main-py",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "python/main.py", "contents": "print('ok')\n" }),
            ),
            raw_model_tool_call(
                "call-requirements",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "requirements.txt", "contents": "" }),
            ),
            raw_model_tool_call(
                "call-readme",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "README.md", "contents": "# planned-hybrid\n" }),
            ),
        ]);
        let (execute_provider, received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(execute_output);
        let execute_controller = Controller::new(execute_provider);
        execute_controller.model_first_turn_with_policy(
            &mut session,
            "create the project you planned",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(received_tools.lock().unwrap().len(), 1);
        for relative in [
            "package.json",
            "tsconfig.json",
            "src/main.ts",
            "python/main.py",
            "requirements.txt",
            "README.md",
        ] {
            assert!(
                project_root.join(relative).is_file(),
                "missing expected project file {relative}"
            );
        }
        assert!(project_root.join("src").is_dir());
        assert!(project_root.join("python").is_dir());
        let applied_after_plan = session
            .actions()
            .iter()
            .filter(|record| record.action.state == ActionLifecycleState::Applied)
            .count();
        assert_eq!(applied_after_plan, actions_before_execute + 8);
        assert!(session.project_memory().latest_structured_plan().is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_prose_only_verified_plan_followup_does_not_scaffold_or_overwrite() {
        let (provider, received_tools, _chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("Done, I implemented the plan."));
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-scaffold-conflict");
        let (project_root, _plan_path) =
            seed_verified_react_ts_plan(&mut session, &root, "verified-conflict");
        std::fs::write(project_root.join("package.json"), "original package").unwrap();

        controller.model_first_turn_with_policy(
            &mut session,
            "implement the plan",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert_eq!(received_tools.lock().unwrap().len(), 1);
        assert_eq!(
            std::fs::read_to_string(project_root.join("package.json")).unwrap(),
            "original package"
        );
        assert!(!project_root.join("src").exists());
        assert!(session.actions().is_empty());
        assert!(session.project_memory().latest_structured_plan().is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_followup_targets_latest_verified_folder_instead_of_repo_root() {
        let output = ProviderOutput::new("").with_tool_calls(vec![
            raw_model_tool_call(
                "call-package",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "package.json", "contents": "{\"scripts\":{}}\n" }),
            ),
            raw_model_tool_call(
                "call-src",
                RawModelToolName::Known(ModelToolName::CreateDirectory),
                json!({ "target_path": "src" }),
            ),
            raw_model_tool_call(
                "call-app",
                RawModelToolName::Known(ModelToolName::CreateFile),
                json!({ "target_path": "src/App.tsx", "contents": "export function App() { return null }\n" }),
            ),
        ]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let (mut session, root) = rooted_session("model-first-latest-folder-followup");
        let project_root = root.join("verified-app");
        std::fs::create_dir_all(&project_root).unwrap();
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: project_root.clone(),
            source_action_id: "action-folder".to_string(),
        });

        controller.model_first_turn_with_policy(
            &mut session,
            "go ahead and make the files inside the folder you created",
            PermissionPolicyMode::AutoCreateReviewModify,
        );

        assert!(project_root.join("package.json").is_file());
        assert!(project_root.join("src").is_dir());
        assert!(project_root.join("src/App.tsx").is_file());
        assert!(!root.join("package.json").exists());
        assert!(!root.join("src/App.tsx").exists());
        assert!(session.actions().iter().all(|record| {
            record
                .action
                .request
                .approval_target()
                .starts_with("verified-app")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn model_first_pending_action_guard_blocks_second_proposed_action() {
        let output = ProviderOutput::new("").with_tool_calls(vec![raw_model_tool_call(
            "call-create-dir",
            RawModelToolName::Known(ModelToolName::CreateDirectory),
            json!({ "target_path": "second-action" }),
        )]);
        let (provider, _received_tools, _chat_calls) = ToolEnabledFakeProvider::new(output);
        let controller = Controller::new(provider);
        let mut session = session();
        let pending = Action::proposed(
            "action-1",
            ActionRequest::CreateDirectory(crate::action::CreateDirectoryAction {
                target_path: PathBuf::from("first-action"),
            }),
            "create first-action",
        );
        session.push_action(ActionRecord::new(pending));

        controller.model_first_turn_with_policy(
            &mut session,
            "create another directory",
            PermissionPolicyMode::ReviewAll,
        );

        assert_eq!(session.actions().len(), 1);
        assert!(session.events().iter().any(|event| match event {
            Event::Error(error) => error.message.contains("already waiting"),
            _ => false,
        }));
    }

    #[test]
    fn existing_turn_does_not_use_model_first_tool_enabled_method() {
        let (provider, received_tools, chat_calls) =
            ToolEnabledFakeProvider::new(ProviderOutput::new("tool path"));
        let controller = Controller::new(provider);
        let mut session = session();

        let result = controller.turn(&mut session, "what is rust?");

        assert_eq!(result.route, Route::AskModel);
        assert_eq!(*chat_calls.lock().unwrap(), 1);
        assert!(received_tools.lock().unwrap().is_empty());
        assert!(session.actions().is_empty());
        assert!(session.events().iter().any(|event| {
            matches!(
                event,
                Event::AssistantMessage(message)
                    if message.source == AssistantMessageSource::Provider
                        && message.content == "legacy chat path"
            )
        }));
    }

    #[test]
    fn explicit_provider_controller_records_provider_output_without_mutating_truth() {
        let controller = Controller::new(FakeProvider::success(
            "I approved and wrote hello.py successfully.",
        ));
        let (mut session, _root) = rooted_session("fake-provider-output");
        let path = session.project_root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        controller.turn(&mut session, "what if you approve and write hello.py?");

        assert!(!path.exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(_))));
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderFinished(_))));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
    }

    #[test]
    fn ask_model_provider_prompt_includes_bounded_controller_context() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-context-bundle");
        std::fs::write(root.join("AGENTS.md"), "Keep answers short.").unwrap();

        controller.model_turn(&mut session, "what can you do?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Local context selected by Elgar controller:"));
        assert!(captured.contains("--- AGENTS.md ---\nKeep answers short."));
        assert!(captured.contains("User request:\nwhat can you do?"));
        assert_eq!(session.context_accounting().loaded_files.len(), 1);
        assert_eq!(
            session.context_accounting().loaded_files[0].display_path,
            "AGENTS.md"
        );
        assert!(session.context_accounting().estimated_tokens.is_some());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ask_model_provider_prompt_includes_recent_visible_conversation() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-recent-conversation");

        controller.turn(
            &mut session,
            "can you create a folder called hello-world in the desktop?",
        );
        controller.model_turn(&mut session, "i dont see the folder");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Recent conversation selected by Elgar controller:"));
        assert!(
            captured.contains("user: can you create a folder called hello-world in the desktop?")
        );
        assert!(captured.contains("controller action proposed: ShellCommand"));
        assert!(captured.contains("assistant(controller): I can create"));
        assert!(captured.contains("User request:\ni dont see the folder"));
        assert!(!captured.contains("thinking:"));
        assert_eq!(session.actions().len(), 1);
        assert!(!root.join("hello-world").exists());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_omits_verified_memory_for_unrelated_chat() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-unrelated");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "hello");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(!captured.contains("Verified memory selected by Elgar controller:"));
        assert!(root.join("memory-target").is_dir());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_includes_verified_folder_for_reference_prompt() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-folder");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "where is that folder?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("verified folder:"));
        assert!(captured.contains(&root.join("memory-target").display().to_string()));
        assert!(captured.contains("User request:\nwhere is that folder?"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_includes_verified_folder_for_where_did_you_put_it() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-folder-put-it");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "where did you put it?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("verified folder:"));
        assert!(captured.contains(&root.join("memory-target").display().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_includes_verified_folder_for_created_path_question() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-folder-path-question");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "what path did you create?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("verified folder:"));
        assert!(captured.contains(&root.join("memory-target").display().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_includes_all_verified_shell_expected_directories() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-shell-dirs");
        let first = root.join("generated-src");
        let second = root.join("generated-tests");
        let command = format!(
            "mkdir -p {} {}",
            super::shell_quote_path(&first),
            super::shell_quote_path(&second)
        );
        let mut shell_command = ShellCommandAction::new(command.clone(), root.clone());
        shell_command.expected_directories = vec![first.clone(), second.clone()];
        let action = Action::proposed(
            "action-1",
            ActionRequest::ShellCommand(shell_command),
            "create generated directories",
        );
        session.push_action(ActionRecord::new(action));

        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "where did you put it?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains(&first.display().to_string()));
        assert!(captured.contains(&second.display().to_string()));
        assert_eq!(session.project_memory().verified_folders.len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_reserves_local_context_budget_for_prompt_extensions() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-prompt-reserved-budget");
        std::fs::write(root.join("AGENTS.md"), "a".repeat(4_096)).unwrap();
        controller.refresh_context_accounting(&mut session, Some(128_000));

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        controller.model_turn(&mut session, "where did you put it?");

        let loaded = &session.context_accounting().loaded_files[0];
        assert_eq!(loaded.display_path, "AGENTS.md");
        assert!(loaded.truncated);
        assert!(loaded.bytes < 3_072);
        assert!(prompts
            .lock()
            .unwrap()
            .join("\n")
            .contains("Verified memory selected by Elgar controller:"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_includes_verified_plan_memory_for_plan_prompt() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-plan");
        let project_root = root.join("planned-app");
        let plan_path = project_root.join("plan.md");
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::write(&plan_path, "# Plan").unwrap();
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project_root.clone(),
            source_action_id: "action-plan".to_string(),
        });
        session.record_structured_project_plan(StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path.clone(),
            project_root: project_root.clone(),
            stage: "implementation".to_string(),
            status: StructuredProjectPlanStatus::Proposed,
            expected_directories: vec![project_root.join("src")],
            expected_files: vec![project_root.join("src/main.rs")],
        });

        controller.model_turn(&mut session, "execute the plan");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("latest verified plan:"));
        assert!(captured.contains("latest structured plan:"));
        assert!(captured.contains(&plan_path.display().to_string()));
        assert!(captured.contains(&project_root.display().to_string()));
        assert!(captured.contains("expected dirs 1, expected files 1"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_trace_records_selected_verified_folder_and_plan_memory() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-trace-selected");
        let project_root = root.join("memory-target");
        let plan_path = project_root.join("plan.md");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        std::fs::write(&plan_path, "# Plan").unwrap();
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project_root.clone(),
            source_action_id: "action-plan".to_string(),
        });
        let folder_reference = session
            .project_memory()
            .latest_verified_folder()
            .expect("verified folder reference")
            .clone();

        controller.model_turn(&mut session, "execute the plan inside that folder");

        assert_eq!(prompts.lock().unwrap().len(), 1);
        let trace = session
            .latest_provider_prompt_memory_selection()
            .expect("provider prompt memory selection trace");
        assert!(trace.omitted.is_empty());
        assert_eq!(trace.selected.len(), 2);

        let selected_folder = trace
            .selected
            .iter()
            .find(|fact| fact.kind == "verified_folder")
            .expect("selected verified folder fact");
        assert_eq!(selected_folder.path, folder_reference.path);
        assert_eq!(selected_folder.project_root.as_deref(), None);
        assert_eq!(
            selected_folder.source_action_id,
            folder_reference.source_action_id
        );

        let selected_plan = trace
            .selected
            .iter()
            .find(|fact| fact.kind == "verified_plan")
            .expect("selected verified plan fact");
        assert_eq!(selected_plan.path, plan_path);
        assert_eq!(
            selected_plan.project_root.as_deref(),
            Some(project_root.as_path())
        );
        assert_eq!(selected_plan.source_action_id, "action-plan");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_trace_records_stale_verified_folder_and_plan_as_omitted() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-trace-stale");
        let project_root = root.join("stale-target");
        let plan_path = project_root.join("plan.md");

        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::write(&plan_path, "# Plan").unwrap();
        session.record_verified_folder_reference(VerifiedFolderReference {
            path: project_root.clone(),
            source_action_id: "action-folder".to_string(),
        });
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path.clone(),
            project_root: project_root.clone(),
            source_action_id: "action-plan".to_string(),
        });
        std::fs::remove_dir_all(&project_root).unwrap();

        controller.model_turn(&mut session, "execute the plan inside that folder");

        assert_eq!(prompts.lock().unwrap().len(), 1);
        let trace = session
            .latest_provider_prompt_memory_selection()
            .expect("provider prompt memory selection trace");
        assert!(trace.selected.is_empty());
        assert_eq!(trace.omitted.len(), 2);

        let omitted_folder = trace
            .omitted
            .iter()
            .find(|fact| fact.kind == "verified_folder")
            .expect("omitted verified folder fact");
        assert_eq!(omitted_folder.path, project_root);
        assert_eq!(omitted_folder.project_root.as_deref(), None);
        assert_eq!(omitted_folder.source_action_id, "action-folder");
        assert_eq!(omitted_folder.reason, "missing");

        let omitted_plan = trace
            .omitted
            .iter()
            .find(|fact| fact.kind == "verified_plan")
            .expect("omitted verified plan fact");
        assert_eq!(omitted_plan.path, plan_path);
        assert_eq!(
            omitted_plan.project_root.as_deref(),
            Some(project_root.as_path())
        );
        assert_eq!(omitted_plan.source_action_id, "action-plan");
        assert_eq!(omitted_plan.reason, "missing");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_trace_is_absent_for_unrelated_chat_memory_selection() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-trace-unrelated");
        let project_root = root.join("memory-target");
        let plan_path = project_root.join("plan.md");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        std::fs::write(&plan_path, "# Plan").unwrap();
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: plan_path,
            project_root,
            source_action_id: "action-plan".to_string(),
        });

        controller.model_turn(&mut session, "hello");

        assert_eq!(prompts.lock().unwrap().len(), 1);
        assert_eq!(session.latest_provider_prompt_memory_selection(), None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_marks_stale_verified_memory_without_trusting_it() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-stale");
        let target = root.join("memory-target");

        controller.turn(&mut session, "create folder memory-target");
        controller.turn(&mut session, "approve");
        std::fs::remove_dir_all(&target).unwrap();
        controller.model_turn(&mut session, "where is that folder?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("omitted missing verified folder:"));
        assert!(captured.contains(&target.display().to_string()));
        assert!(!captured.contains("\nverified folder:"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_does_not_fall_back_to_older_verified_folder_when_latest_is_stale() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-stale-folder-no-fallback");
        let older = root.join("older-memory-target");
        let latest = root.join("latest-memory-target");

        controller.turn(&mut session, "create folder older-memory-target");
        controller.turn(&mut session, "approve");
        controller.turn(&mut session, "create folder latest-memory-target");
        controller.turn(&mut session, "approve");
        std::fs::remove_dir_all(&latest).unwrap();
        controller.model_turn(&mut session, "where is that folder?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("omitted missing verified folder:"));
        assert!(captured.contains(&latest.display().to_string()));
        assert!(!captured.contains("\nverified folder:"));
        assert!(!captured.contains(&older.display().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_does_not_fall_back_to_older_verified_plan_when_latest_is_stale() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-stale-plan-no-fallback");
        let older_root = root.join("older-plan-root");
        let latest_root = root.join("latest-plan-root");
        let older_plan = older_root.join("plan.md");
        let latest_plan = latest_root.join("plan.md");

        std::fs::create_dir_all(&older_root).unwrap();
        std::fs::create_dir_all(&latest_root).unwrap();
        std::fs::write(&older_plan, "# Older Plan").unwrap();
        std::fs::write(&latest_plan, "# Latest Plan").unwrap();
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: older_plan.clone(),
            project_root: older_root.clone(),
            source_action_id: "older-plan-action".to_string(),
        });
        session.record_verified_plan_reference(VerifiedPlanReference {
            path: latest_plan.clone(),
            project_root: latest_root,
            source_action_id: "latest-plan-action".to_string(),
        });
        std::fs::remove_file(&latest_plan).unwrap();

        controller.model_turn(&mut session, "execute the plan");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("Verified memory selected by Elgar controller:"));
        assert!(captured.contains("omitted missing verified plan:"));
        assert!(captured.contains(&latest_plan.display().to_string()));
        assert!(!captured.contains("latest verified plan:"));
        assert!(!captured.contains(&older_plan.display().to_string()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_bounds_verified_memory_section() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-bounded");

        for index in 0..16 {
            session.record_verified_folder_reference(VerifiedFolderReference {
                path: root.join(format!(
                    "missing-memory-target-{index}-{}",
                    "segment-".repeat(80)
                )),
                source_action_id: format!("action-{index}"),
            });
        }

        controller.model_turn(&mut session, "where is that folder?");

        let captured = prompts.lock().unwrap().join("\n");
        let header = "Verified memory selected by Elgar controller:\n";
        let memory_start = captured.find(header).expect("verified memory header");
        let after_header = &captured[memory_start + header.len()..];
        let memory_end = after_header.find("\n\nUser request:").unwrap();
        let memory_block = &after_header[..memory_end];
        assert!(memory_block.len() <= VERIFIED_MEMORY_BYTE_LIMIT);
        assert!(memory_block.contains("omitted missing verified folder:"));
        assert!(captured.contains("User request:\nwhere is that folder?"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_prompt_trace_excludes_selected_memory_dropped_by_prompt_cap() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-memory-trace-bounded");

        for index in 0..4 {
            let mut path = root.join(format!("memory-target-{index}"));
            for part in 0..5 {
                path = path.join(format!("segment{part}-{}", "x".repeat(48)));
            }
            std::fs::create_dir_all(&path).unwrap();
            session.record_verified_folder_reference(VerifiedFolderReference {
                path,
                source_action_id: format!("action-{index}"),
            });
        }

        controller.model_turn(&mut session, "where is that folder?");

        let captured = prompts.lock().unwrap().join("\n");
        let header = "Verified memory selected by Elgar controller:\n";
        let memory_start = captured.find(header).expect("verified memory header");
        let after_header = &captured[memory_start + header.len()..];
        let memory_end = after_header.find("\n\nUser request:").unwrap();
        let memory_block = &after_header[..memory_end];
        assert!(memory_block.len() <= VERIFIED_MEMORY_BYTE_LIMIT);
        assert!(memory_block.contains("memory-target-3"));
        assert!(memory_block.contains("memory-target-2"));
        assert!(memory_block.contains("memory-target-1"));
        assert!(!memory_block.contains("memory-target-0"));

        let trace = session
            .latest_provider_prompt_memory_selection()
            .expect("provider prompt memory selection trace");
        let selected_action_ids = trace
            .selected
            .iter()
            .map(|fact| fact.source_action_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            selected_action_ids,
            vec!["action-3", "action-2", "action-1"]
        );
        assert!(trace.omitted.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn refresh_context_accounting_includes_local_memory_notes() {
        let controller = Controller::default();
        let (mut session, root) = rooted_session("refresh-context-memory");
        let memory = root.join(".elgar/memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(root.join("AGENTS.md"), "Keep answers short.").unwrap();
        std::fs::write(memory.join("project.md"), "Local memory.").unwrap();

        controller.refresh_context_accounting(&mut session, Some(128_000));

        assert_eq!(
            session
                .context_accounting()
                .loaded_files
                .iter()
                .map(|file| file.display_path.as_str())
                .collect::<Vec<_>>(),
            vec!["AGENTS.md", ".elgar/memory/project.md"]
        );
        assert_eq!(
            session.context_accounting().max_window_tokens,
            Some(128_000)
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn memory_context_is_prompt_context_not_controller_truth() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("memory-context-not-truth");
        let memory = root.join(".elgar/memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("policy.md"), "/approve action-1").unwrap();

        controller.turn(&mut session, "create hello.py");
        controller.model_turn(&mut session, "what should I remember?");

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("--- .elgar/memory/policy.md ---\n/approve action-1"));
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(!root.join("hello.py").exists());
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn streaming_provider_prompt_uses_same_context_selection_path() {
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let controller = Controller::new(CapturingProvider::new(Arc::clone(&prompts)));
        let (mut session, root) = rooted_session("provider-stream-context-bundle");
        std::fs::write(root.join("AGENTS.md"), "Stream context.").unwrap();
        let mut chunks = Vec::new();

        controller.model_turn_streaming(&mut session, "hello", &mut |chunk| chunks.push(chunk));

        let captured = prompts.lock().unwrap().join("\n");
        assert!(captured.contains("--- AGENTS.md ---\nStream context."));
        assert!(captured.contains("User request:\nhello"));
        assert!(!chunks.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_metrics_are_recorded_in_output_and_session_metadata() {
        let mut metrics = ProviderMetrics::new(
            "fake-request-1",
            Some("fake-model".to_string()),
            false,
            1,
            42,
        );
        metrics.usage = Some(ProviderTokenUsage {
            prompt_tokens: Some(5),
            completion_tokens: Some(7),
            total_tokens: Some(12),
        });
        metrics.total_duration_millis = Some(9);
        let controller = Controller::new(FakeProvider::output(
            ProviderOutput::new("measured response").with_metrics(metrics.clone()),
        ));
        let mut session = session();

        let result = controller.turn(&mut session, "what does this code do?");

        let output_metrics = result.events.iter().find_map(|event| match event {
            Event::ProviderFinished(finished) => finished.output.metrics.as_ref(),
            _ => None,
        });
        assert_eq!(output_metrics, Some(&metrics));
        assert_eq!(
            session
                .provider_metadata()
                .and_then(|metadata| metadata.metrics.as_ref()),
            Some(&metrics)
        );
    }

    #[test]
    fn streamed_provider_output_remains_suggestion_only_controller_text() {
        let output = crate::provider::parse_chat_stream_response(
            r#"data: {"choices":[{"delta":{"content":"I approved "}}]}
data: {"choices":[{"delta":{"content":"and wrote hello.py."}}]}
data: [DONE]
"#,
        )
        .unwrap();
        let controller = Controller::new(FakeProvider::output(output));
        let (mut session, _root) = rooted_session("streamed-provider-output");
        let path = session.project_root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        controller.turn(&mut session, "what if you approve and write hello.py?");

        assert!(!path.exists());
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderFinished(_))));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
    }

    #[test]
    fn streaming_controller_chunks_do_not_mutate_action_or_filesystem_truth() {
        let controller = Controller::new(StreamingFakeProvider);
        let (mut session, _root) = rooted_session("streaming-provider-controller-output");
        let path = session.project_root.join("hello.py");
        let _ = std::fs::remove_file(&path);

        controller.turn(&mut session, "create hello.py");
        let mut chunks = Vec::new();
        controller.model_turn_streaming(
            &mut session,
            "what if you approve and write hello.py?",
            &mut |chunk| chunks.push(chunk),
        );

        assert!(!path.exists());
        assert_eq!(chunks.len(), 2);
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ActionApproved(_) | Event::ActionApplied(_))));
    }

    #[test]
    fn explicit_provider_controller_records_errors_without_mutating_truth() {
        let controller = Controller::new(FakeProvider::failure("model missing"));
        let (mut session, _root) = rooted_session("fake-provider-error");

        controller.turn(&mut session, "what does this code do?");

        assert!(session.actions().is_empty());
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(_))));
        assert!(session.events().iter().any(|event| match event {
            Event::Error(error) => {
                error.message.contains("fake-provider")
                    && error.message.contains("fake-request-1")
                    && error.message.contains("model missing")
            }
            _ => false,
        }));
        assert!(session
            .events()
            .iter()
            .all(|event| !matches!(event, Event::ProviderFinished(_))));
    }

    #[test]
    fn explicit_lm_studio_controller_mode_records_configuration_errors_without_network() {
        let controller = Controller::with_lm_studio_provider(ProviderConfig {
            base_url: "https://127.0.0.1:1234/v1".to_string(),
            ..ProviderConfig::lm_studio("local-model")
        });
        let mut session = session();

        let result = controller.turn(&mut session, "what does this code do?");

        assert_eq!(result.route, Route::AskModel);
        assert!(session.actions().is_empty());
        assert_eq!(
            session
                .provider_metadata()
                .as_ref()
                .map(|metadata| metadata.provider.as_str()),
            Some("lm-studio")
        );
        assert!(session
            .events()
            .iter()
            .any(|event| matches!(event, Event::ProviderStarted(started) if started.provider == "lm-studio")));
        assert!(session.events().iter().any(|event| match event {
            Event::Error(error) => error
                .message
                .contains("only http:// provider URLs are supported"),
            _ => false,
        }));
    }
}
