use crate::{
    event::{Event, VerifiedActionResult},
    session::Session,
};

pub fn placeholder_message() -> &'static str {
    "Elgar v0.2 is ready. Run `elgar` from an interactive terminal for the TUI, or pass a prompt/subcommand."
}

pub fn render_session(session: &Session) -> String {
    session
        .events()
        .iter()
        .map(render_event)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_event(event: &Event) -> String {
    match event {
        Event::UserMessage(message) => format!("user: {}", message.content),
        Event::AssistantMessage(message) => {
            format!("assistant {:?}: {}", message.source, message.content)
        }
        Event::ProviderStarted(started) => {
            format!(
                "provider started: {} request {}",
                started.provider, started.request_id
            )
        }
        Event::ProviderFinished(finished) => {
            format!(
                "provider finished: {} request {}",
                finished.provider, finished.request_id
            )
        }
        Event::ActionProposed(action) => {
            format!(
                "action proposed: {} {:?} {}",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionApproved(action) => {
            format!(
                "action approved: {} {:?} {}",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionRejected(action) => {
            format!(
                "action rejected: {} {:?} {}",
                action.action_id, action.action_kind, action.summary
            )
        }
        Event::ActionApplied(applied) => {
            format!(
                "action applied: {} {:?} {}",
                applied.action_id,
                applied.action_kind,
                render_verified_result(&applied.result)
            )
        }
        Event::ActionFailed(failed) => {
            format!(
                "action failed: {} {:?} {}",
                failed.action_id, failed.action_kind, failed.reason
            )
        }
        Event::Error(error) => format!("error: {}", error.message),
    }
}

fn render_verified_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => format!("file written: {path}"),
        VerifiedActionResult::File(file) => format!("file result: {file:?}"),
        VerifiedActionResult::Shell(shell) => {
            let mut rendered = format!(
                "shell result: exit code {:?}, timed_out={}, stdout_truncated={}, stderr_truncated={}",
                shell.exit_code, shell.timed_out, shell.stdout_truncated, shell.stderr_truncated
            );
            if let Some(effect) = &shell.verified_effect {
                rendered.push_str(&format!(", {effect}"));
            }
            rendered
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        action::Action,
        event::{ActionApplied, ActionEvent, Event, VerifiedActionResult},
        session::{ActionRecord, Session},
    };

    use super::render_session;

    #[test]
    fn reports_action_lifecycle_states() {
        let mut session = Session::new("session-1", ".", ".");
        let action = Action::proposed_write_file("action-1", "hello.py", "", "write hello.py");
        session.push_action(ActionRecord::new(action.clone()));
        session.push_event(Event::ActionProposed(ActionEvent::new(
            action.id.clone(),
            action.kind(),
            action.summary.clone(),
        )));
        session.push_event(Event::ActionRejected(ActionEvent::new(
            action.id.clone(),
            action.kind(),
            action.summary.clone(),
        )));
        session.push_event(Event::ActionApplied(ActionApplied::new(
            action.id.clone(),
            action.kind(),
            VerifiedActionResult::FileWritten {
                path: "hello.py".to_string(),
            },
        )));

        let rendered = render_session(&session);

        assert!(rendered.contains("action proposed: action-1 CreateFile write hello.py"));
        assert!(rendered.contains("action rejected: action-1 CreateFile write hello.py"));
        assert!(rendered.contains("action applied: action-1 CreateFile file written: hello.py"));
    }
}
