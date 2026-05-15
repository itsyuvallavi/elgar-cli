use serde::{Deserialize, Serialize};

use crate::{
    action::Action,
    event::{
        ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource, Event,
        ProviderFinished, ProviderStarted, UserMessage,
    },
    fs::Filesystem,
    provider::ProviderStub,
    router::{route_input, Route},
    session::{ActionRecord, ProviderMetadata, Session},
};

/// First controller turn flow over a deterministic provider stub.
///
/// The controller records facts into session state. It does not execute actions,
/// mutate files, call a network provider, or treat provider text as truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Controller {
    pub provider: ProviderStub,
}

impl Controller {
    pub fn new(provider: ProviderStub) -> Self {
        Self { provider }
    }

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
            Route::ProposeWriteFile => self.handle_propose_write_file(session, input),
        }

        TurnResult {
            route,
            events: session.events()[start_index..].to_vec(),
        }
    }

    fn handle_ask_model(&self, session: &mut Session, input: &str) {
        let response = self.provider.ask(input);

        let mut metadata = ProviderMetadata::new(response.provider.clone());
        metadata.model = response.model.clone();
        metadata.request_id = Some(response.request_id.clone());
        session.set_provider_metadata(metadata);

        session.push_event(Event::ProviderStarted(ProviderStarted::new(
            response.provider.clone(),
            response.request_id.clone(),
        )));
        session.push_event(Event::ProviderFinished(ProviderFinished::new(
            response.provider,
            response.request_id,
            response.output.clone(),
        )));
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            response.output.text,
            AssistantMessageSource::Provider,
        )));
    }

    fn handle_propose_write_file(&self, session: &mut Session, input: &str) {
        let Some(target_path) = parse_write_file_target(input) else {
            push_controller_message(
                session,
                "WriteFile request was recognized, but no target path could be parsed.",
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
            "Proposed WriteFile action. Approve or reject before any file is written.",
        );
    }

    fn handle_reject_action(&self, session: &mut Session) {
        let Some(index) = latest_proposed_action_index(session) else {
            push_controller_message(session, "No proposed action is waiting for rejection.");
            return;
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
        let Some(index) = latest_proposed_action_index(session) else {
            push_controller_message(session, "No proposed action is waiting for approval.");
            return;
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

        match Filesystem::apply_write_file(&approved, &session.project_root) {
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
                    "Applied approved WriteFile action and verified the target file exists.",
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
                push_controller_message(session, "Approved WriteFile action failed. No verified file-written result was recorded.");
            }
        }
    }
}

impl Default for Controller {
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
    "Elgar core harness can classify help, model questions, write-file requests, approvals, and rejections.";
const UNKNOWN_MESSAGE: &str =
    "Input was not recognized. No provider, file, action, or shell operation was run.";

fn push_controller_message(session: &mut Session, message: &'static str) {
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
}

fn next_action_id(session: &Session) -> String {
    format!("action-{}", session.actions().len() + 1)
}

fn latest_proposed_action_index(session: &Session) -> Option<usize> {
    session
        .actions()
        .iter()
        .rposition(|record| record.action.state == crate::action::ActionLifecycleState::Proposed)
}

fn action_target_label(action: &Action) -> String {
    match &action.request {
        crate::action::ActionRequest::WriteFile(write_file) => {
            write_file.target_path.display().to_string()
        }
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
        action::ActionLifecycleState,
        event::{AssistantMessageSource, Event, VerifiedActionResult},
        provider::ProviderStub,
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
}
