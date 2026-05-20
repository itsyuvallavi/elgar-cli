use serde::{Deserialize, Serialize};

use crate::{
    action::Action,
    context::ContextAccounting,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
        ErrorEvent, Event, ProviderFinished, ProviderStarted, UserMessage,
    },
    fs::Filesystem,
    provider::{
        ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStreamChunk, ProviderStub,
    },
    router::{route_input, Route},
    session::{ActionRecord, PendingActionSelection, ProviderMetadata, Session},
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

        let route = route_input(input);
        match route {
            Route::AskModel => self.handle_ask_model(session, input),
            Route::Help => push_controller_message(session, HELP_MESSAGE),
            Route::Unknown => push_controller_message(session, UNKNOWN_MESSAGE),
            Route::ApproveAction => self.handle_approve_action(session),
            Route::RejectAction => self.handle_reject_action(session),
            Route::ProposeMarkdownPlanFile => {
                self.handle_propose_markdown_plan_file(session, input)
            }
            Route::ProposeWriteFile => self.handle_propose_write_file(session, input),
            Route::ProposePatchFile => self.handle_propose_patch_file(session, input),
            Route::ProposeOverwriteFile => self.handle_propose_overwrite_file(session, input),
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

        let mut metadata = ProviderMetadata::new(request.provider.clone());
        metadata.model = request.model.clone();
        metadata.request_id = Some(request.request_id.clone());
        session.set_provider_metadata(metadata);

        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            request.provider.clone(),
            request.request_id.clone(),
        )));

        match self
            .provider
            .chat_stream_with_metadata(input, &request, on_chunk)
        {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    let mut metadata = ProviderMetadata::new(request.provider.clone());
                    metadata.model = request.model.clone();
                    metadata.request_id = Some(request.request_id.clone());
                    metadata.metrics = Some(metrics);
                    session.set_provider_metadata(metadata);
                }
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider,
                    request.request_id,
                    output.clone(),
                )));
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    output.text,
                    AssistantMessageSource::Provider,
                )));
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
            }
        }
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

        let target_path = parse_markdown_plan_target(input);
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
        match self.provider.chat_with_metadata(&prompt, &request) {
            Ok(output) => {
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
                session.push_event(Event::AssistantMessage(AssistantMessage::new(
                    provider_text,
                    AssistantMessageSource::Provider,
                )));

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
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} provider request {} failed: {error}",
                    request.provider, request.request_id
                ))));
            }
        }
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

        match Filesystem::apply_file_action(&approved, &session.project_root) {
            Ok(result) => {
                let record = session
                    .action_mut(index)
                    .expect("approved action index must reference an action record");
                record.verified_result = Some(result.clone());
                record.failure_reason = None;
                record.action = approved.mark_applied();
                session.push_event(Event::ActionApplied(ActionApplied::new(
                    approved.id.clone(),
                    approved.kind(),
                    result,
                )));
                push_controller_message(
                    session,
                    "Applied approved file action and verified the expected file contents.",
                );
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
                push_controller_message(
                    session,
                    "Approved file action failed. No verified filesystem result was recorded.",
                );
            }
        }
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
    "Elgar core harness can classify help, model questions, file-action requests, approvals, and rejections.";
const UNKNOWN_MESSAGE: &str =
    "Input was not recognized. No provider, file, action, or shell operation was run.";
const AMBIGUOUS_PENDING_ACTION_MESSAGE: &str =
    "Multiple proposed actions are waiting. Elgar will not approve, reject, or create another action until this session is repaired.";

fn push_controller_message(session: &mut Session, message: &'static str) {
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
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

fn parse_markdown_plan_target(input: &str) -> std::path::PathBuf {
    if let Some(explicit_target) = explicit_markdown_target(input) {
        return explicit_target.into();
    }

    std::path::PathBuf::from(format!("{}-plan.md", markdown_plan_slug(input)))
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
        "a", "an", "and", "build", "create", "draft", "file", "make", "markdown", "md", "plan",
        "please", "the", "use", "using", "with", "write",
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
    let trimmed = input.trim();
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
    use std::path::PathBuf;

    use crate::{
        action::{ActionLifecycleState, FileActionVerification},
        event::{
            AssistantMessageSource, Event, ProviderMetrics, ProviderOutput, ProviderTokenUsage,
            VerifiedActionResult,
        },
        provider::{ControllerProvider, ProviderConfig, ProviderError, ProviderStub},
        router::Route,
        session::Session,
    };

    use super::Controller;

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
