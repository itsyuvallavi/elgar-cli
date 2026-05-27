use serde::{Deserialize, Serialize};

use crate::{
    context::ContextAccounting,
    controller_provider::{
        push_provider_message_if_visible, record_provider_request_metadata,
        set_provider_metrics_metadata,
    },
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderFinished,
        ProviderStarted, UserMessage,
    },
    provider::{
        ControllerProvider, LmStudioProvider, ProviderConfig, ProviderStreamChunk, ProviderStub,
    },
    router::{normalize_pasted_transcript_input, route_input, Route},
    session::Session,
};

/// Small compatibility controller for explicit provider chat and local slash
/// commands.
///
/// Normal TUI/CLI turns use `AgentRuntime`. Permissioned filesystem changes
/// are applied by `ActionGate` after typed runtime proposals.
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
        let route = route_input(normalized_input.as_ref());
        match route {
            Route::AskModel => self.handle_model_turn_after_user_event(session, input),
            Route::Help => push_controller_message(session, HELP_MESSAGE),
            Route::Unknown => push_controller_message(session, UNKNOWN_MESSAGE),
            Route::ApproveAction => push_controller_message(
                session,
                "Action approval is handled by the explicit /approve command path.",
            ),
            Route::RejectAction => push_controller_message(
                session,
                "Action rejection is handled by the explicit /reject command path.",
            ),
            Route::ProposeMarkdownPlanFile
            | Route::ProposeWriteFile
            | Route::ProposePatchFile
            | Route::ProposeOverwriteFile
            | Route::ProposeDeleteFile
            | Route::ProposeMoveFile
            | Route::ProposeCreateDirectory
            | Route::ProposeShellCommand
            | Route::ExecutePlan => push_controller_message(
                session,
                "Action routes are handled by AgentRuntime tool turns, not Controller text routing.",
            ),
        }

        TurnResult {
            route,
            events: session.events()[start_index..].to_vec(),
        }
    }

    /// Record an explicit chat turn without asking the router to classify text.
    pub fn model_turn(&self, session: &mut Session, input: &str) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));
        self.handle_model_turn_after_user_event(session, input);

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    pub fn model_turn_streaming(
        &self,
        session: &mut Session,
        input: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> TurnResult {
        let start_index = session.events().len();
        session.push_event(Event::UserMessage(UserMessage::new(input)));

        let request = self.provider.request_metadata();
        session.push_event(Event::ProviderStarted(
            ProviderStarted::new(request.provider.clone(), request.request_id.clone())
                .with_request_details(request.model.clone(), "plain", 0),
        ));
        record_provider_request_metadata(session, &request);

        match self
            .provider
            .chat_stream_with_metadata(input, &request, on_chunk)
        {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    output.clone(),
                )));
                push_provider_message_if_visible(session, output.text);
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} request {} failed: {}",
                    request.provider, request.request_id, error
                ))));
            }
        }

        TurnResult {
            route: Route::AskModel,
            events: session.events()[start_index..].to_vec(),
        }
    }

    fn handle_model_turn_after_user_event(&self, session: &mut Session, input: &str) {
        let request = self.provider.request_metadata();
        session.push_event(Event::ProviderStarted(
            ProviderStarted::new(request.provider.clone(), request.request_id.clone())
                .with_request_details(request.model.clone(), "plain", 0),
        ));
        record_provider_request_metadata(session, &request);

        match self.provider.chat_with_metadata(input, &request) {
            Ok(output) => {
                if let Some(metrics) = output.metrics.clone() {
                    set_provider_metrics_metadata(session, &request, metrics);
                }
                session.push_event(Event::ProviderFinished(ProviderFinished::new(
                    request.provider.clone(),
                    request.request_id.clone(),
                    output.clone(),
                )));
                push_provider_message_if_visible(session, output.text);
            }
            Err(error) => {
                session.push_event(Event::Error(ErrorEvent::new(format!(
                    "{} request {} failed: {}",
                    request.provider, request.request_id, error
                ))));
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
    "Elgar supports provider chat plus explicit slash commands. Use /tool for tool-enabled turns.";
const UNKNOWN_MESSAGE: &str = "Empty input was not sent to the provider.";

fn push_controller_message(session: &mut Session, message: impl Into<String>) {
    session.push_event(Event::AssistantMessage(AssistantMessage::new(
        message,
        AssistantMessageSource::Controller,
    )));
}

#[cfg(test)]
#[path = "controller_tests/mod.rs"]
mod tests;
