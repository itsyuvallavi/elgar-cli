use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    action::{
        Action, ActionRequest, CreateDirectoryAction, DeleteFileAction, MoveFileAction,
        ShellCommandAction, SHELL_COMMAND_DEFAULT_TIMEOUT_SECONDS,
    },
    context::ContextAccounting,
    controller_project_memory::record_verified_project_memory,
    controller_prompt::provider_prompt_with_context,
    controller_provider::{
        push_provider_message_if_visible, record_provider_request_metadata,
        set_provider_metrics_metadata,
    },
    controller_reporting::{
        create_directory_proposal_message, truth_guard_visible_message,
        verified_action_success_message,
    },
    controller_scaffold::{
        build_project_scaffold_plan, controller_owned_verified_plan_scaffold_actions,
        first_existing_scaffold_target,
    },
    controller_shell_commands::{
        dedupe_paths, display_path_list, shell_quote_path, shell_write_file_command,
        shell_write_many_files_command,
    },
    controller_shell_verify::verify_expected_shell_effect,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage,
    },
    fs::Filesystem,
    policy::{ApprovalSource, PermissionPolicyMode, PolicyDecision},
    provider::{
        ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStreamChunk, ProviderStub,
    },
    router::{
        is_prior_project_execution_request, is_project_creation_request,
        normalize_pasted_transcript_input, route_input, strip_action_request_prefixes, Route,
    },
    session::{
        ActionRecord, PendingActionSelection, Session, StructuredProjectPlan,
        StructuredProjectPlanStatus, VerifiedPlanReference,
    },
    shell::ShellExecutor,
};

mod legacy_controller_model_first;

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
        record_provider_request_metadata(session, &request);

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
        let approval_source = policy_decision.approval_source.clone();
        record.policy_decision = Some(policy_decision);
        session.push_action(record);
        let mut event = ActionEvent::new(
            approved.id.clone(),
            approved.kind(),
            approved.summary.clone(),
        )
        .with_target(target_label);
        if let Some(source) = approval_source {
            event = event.with_approval_source(source);
        }
        session.push_event(Event::ActionApproved(event));

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
        record_provider_request_metadata(session, &request);

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
            .with_target(action_target_label(&approved))
            .with_approval_source(ApprovalSource::user()),
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
    let message = truth_guard_visible_message(session, message.into());
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
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

    if matches!(
        action.request,
        ActionRequest::CreateFile(_) | ActionRequest::CreateDirectory(_)
    ) {
        if let Some(home) = home_dir() {
            if target_path.starts_with(&home) {
                return home;
            }
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
    let policy_decision = PolicyDecision::allow_apply(
        mode,
        "safe controller-owned new create action validated by policy",
    );
    let approval_source = policy_decision.approval_source.clone();
    record.policy_decision = Some(policy_decision);
    record.failure_reason = Some(reason.clone());
    session.push_action(record);
    let mut event = ActionEvent::new(
        approved.id.clone(),
        approved.kind(),
        approved.summary.clone(),
    )
    .with_target(target_label);
    if let Some(source) = approval_source {
        event = event.with_approval_source(source);
    }
    session.push_event(Event::ActionApproved(event));
    session.push_event(Event::ActionFailed(ActionFailed::new(
        approved.id.clone(),
        approved.kind(),
        reason,
    )));
    push_controller_message(session, failure_message);
}

fn push_ambiguous_pending_action_message(session: &mut Session) {
    push_controller_message(session, AMBIGUOUS_PENDING_ACTION_MESSAGE);
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
        "i want you to create a directory ",
        "i want you to create a folder ",
        "i want you to make a directory ",
        "i want you to make a folder ",
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
    if let Some(name) = parse_create_directory_home_named_target(rest) {
        return Some(ParsedCreateDirectoryTarget::ShellDirectory(
            home_dir()?.join(name),
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

fn parse_create_directory_home_named_target(rest: &str) -> Option<String> {
    let rest = trim_request_punctuation(rest);
    for prefix in [
        "in ~/",
        "in ~",
        "inside ~/",
        "inside ~",
        "under ~/",
        "under ~",
        "in home directory",
        "in home folder",
        "in my home directory",
        "in my home folder",
        "in home",
        "in my home",
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
#[path = "controller_tests/mod.rs"]
mod tests;
