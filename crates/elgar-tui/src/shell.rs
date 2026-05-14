use elgar_core::{
    controller::{Controller, TurnResult},
    event::{ActionEvent, Event},
    renderer::render_event,
    session::Session,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiShell {
    pub conversation: ConversationPane,
    pub input: InputArea,
    pub status: StatusLine,
    pub pending_action: PendingActionArea,
}

impl TuiShell {
    pub fn new() -> Self {
        Self {
            conversation: ConversationPane::default(),
            input: InputArea::default(),
            status: StatusLine::ready(),
            pending_action: PendingActionArea::default(),
        }
    }

    pub fn regions(&self) -> [LayoutRegion; 4] {
        [
            LayoutRegion::Conversation,
            LayoutRegion::PendingAction,
            LayoutRegion::Status,
            LayoutRegion::Input,
        ]
    }

    pub fn render(&self) -> String {
        [
            render_section(
                LayoutRegion::Conversation.title(),
                &self.conversation.render_body(),
            ),
            render_section(
                LayoutRegion::PendingAction.title(),
                &self.pending_action.render_body(),
            ),
            render_section(LayoutRegion::Status.title(), &self.status.render_body()),
            render_section(LayoutRegion::Input.title(), &self.input.render_body()),
        ]
        .join("\n")
    }

    pub fn consume_session(&mut self, session: &Session) {
        self.consume_events(&session.events);
    }

    pub fn consume_events<'a>(&mut self, events: impl IntoIterator<Item = &'a Event>) {
        for event in events {
            self.consume_event(event);
        }
    }

    pub fn consume_event(&mut self, event: &Event) {
        self.conversation.push_event(event);
        self.status.observe_event(event);
        self.pending_action.observe_event(event);
    }

    pub fn submit_approval(
        &mut self,
        controller: &Controller,
        session: &mut Session,
    ) -> TurnResult {
        self.submit_input(controller, session, "approve")
    }

    pub fn submit_rejection(
        &mut self,
        controller: &Controller,
        session: &mut Session,
    ) -> TurnResult {
        self.submit_input(controller, session, "reject")
    }

    pub fn submit_input(
        &mut self,
        controller: &Controller,
        session: &mut Session,
        input: &str,
    ) -> TurnResult {
        let result = controller.turn(session, input);
        self.consume_events(&result.events);
        result
    }
}

impl Default for TuiShell {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
}

impl ConversationPane {
    pub fn push_event(&mut self, event: &Event) {
        self.lines.push(render_event(event));
    }

    fn render_body(&self) -> String {
        if self.lines.is_empty() {
            "(empty conversation)".to_string()
        } else {
            self.lines.join("\n")
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputArea {
    pub text: String,
}

impl InputArea {
    fn render_body(&self) -> String {
        format!("> {}", self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
}

impl StatusLine {
    pub fn ready() -> Self {
        Self {
            text: "ready".to_string(),
        }
    }

    pub fn observe_event(&mut self, event: &Event) {
        self.text = match event {
            Event::UserMessage(_) => "input recorded".to_string(),
            Event::AssistantMessage(_) => "assistant message".to_string(),
            Event::ProviderStarted(started) => {
                format!(
                    "provider {} request {} started",
                    started.provider, started.request_id
                )
            }
            Event::ProviderFinished(finished) => {
                format!(
                    "provider {} request {} finished",
                    finished.provider, finished.request_id
                )
            }
            Event::ActionProposed(action) => {
                format!("action {} proposed", action.action_id)
            }
            Event::ActionApproved(action) => {
                format!("action {} approved", action.action_id)
            }
            Event::ActionRejected(action) => {
                format!("action {} rejected", action.action_id)
            }
            Event::ActionApplied(action) => {
                format!("action {} applied", action.action_id)
            }
            Event::ActionFailed(action) => {
                format!("action {} failed", action.action_id)
            }
            Event::Error(_) => "error".to_string(),
        };
    }

    fn render_body(&self) -> String {
        self.text.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PendingActionArea {
    pub panel: Option<ActionApprovalPanel>,
}

impl PendingActionArea {
    pub fn observe_event(&mut self, event: &Event) {
        match event {
            Event::ActionProposed(action) => {
                self.panel = Some(ActionApprovalPanel::pending(action));
            }
            Event::ActionApproved(action) => {
                self.update_or_replace(action, ActionPanelState::Approved, None)
            }
            Event::ActionRejected(action) => self.update_or_replace(
                action,
                ActionPanelState::Rejected,
                Some("rejected by user; no filesystem change was made".to_string()),
            ),
            Event::ActionApplied(action) => self.update_result(
                &action.action_id,
                ActionPanelState::Applied,
                Some(render_verified_result(&action.result)),
            ),
            Event::ActionFailed(action) => self.update_result(
                &action.action_id,
                ActionPanelState::Failed,
                Some(action.reason.clone()),
            ),
            _ => {}
        }
    }

    fn update_or_replace(
        &mut self,
        action: &ActionEvent,
        state: ActionPanelState,
        result: Option<String>,
    ) {
        if let Some(panel) = self
            .panel
            .as_mut()
            .filter(|panel| panel.action_id == action.action_id)
        {
            panel.state = state;
            panel.result = result;
        } else {
            let mut panel = ActionApprovalPanel::pending(action);
            panel.state = state;
            panel.result = result;
            self.panel = Some(panel);
        }
    }

    fn update_result(&mut self, action_id: &str, state: ActionPanelState, result: Option<String>) {
        if let Some(panel) = self
            .panel
            .as_mut()
            .filter(|panel| panel.action_id == action_id)
        {
            panel.state = state;
            panel.result = result;
        } else {
            self.panel = Some(ActionApprovalPanel {
                action_id: action_id.to_string(),
                action_type: "unknown".to_string(),
                target: None,
                summary: "not available from this result event".to_string(),
                state,
                result,
            });
        }
    }

    fn render_body(&self) -> String {
        self.panel
            .as_ref()
            .map(ActionApprovalPanel::render)
            .unwrap_or_else(|| "none".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionApprovalPanel {
    pub action_id: String,
    pub action_type: String,
    pub target: Option<String>,
    pub summary: String,
    pub state: ActionPanelState,
    pub result: Option<String>,
}

impl ActionApprovalPanel {
    fn pending(action: &ActionEvent) -> Self {
        Self {
            action_id: action.action_id.clone(),
            action_type: format!("{:?}", action.action_kind),
            target: action.target.clone(),
            summary: action.summary.clone(),
            state: ActionPanelState::Proposed,
            result: None,
        }
    }

    fn render(&self) -> String {
        let mut lines = vec![
            format!("action id: {}", self.action_id),
            format!("action type: {}", self.action_type),
            format!(
                "target: {}",
                self.target.as_deref().unwrap_or("unavailable")
            ),
            format!("summary: {}", self.summary),
            format!("state: {}", self.state.render()),
        ];

        if let Some(result) = &self.result {
            lines.push(format!("result: {result}"));
        }

        lines.push(format!("instructions: {}", self.state.instructions()));
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionPanelState {
    Proposed,
    Approved,
    Applied,
    Rejected,
    Failed,
}

impl ActionPanelState {
    fn render(self) -> &'static str {
        match self {
            Self::Proposed => "pending approval",
            Self::Approved => "approved",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    fn instructions(self) -> &'static str {
        match self {
            Self::Proposed => "type approve to apply or reject to decline",
            Self::Approved => "approval recorded by controller",
            Self::Applied => "verified result recorded by controller",
            Self::Rejected => "rejected actions are terminal; submit a new proposal to reconsider",
            Self::Failed => "failure recorded by controller",
        }
    }
}

fn render_verified_result(result: &elgar_core::event::VerifiedActionResult) -> String {
    match result {
        elgar_core::event::VerifiedActionResult::FileWritten { path } => {
            format!("file written: {path}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutRegion {
    Conversation,
    Input,
    Status,
    PendingAction,
}

impl LayoutRegion {
    pub fn title(self) -> &'static str {
        match self {
            Self::Conversation => "Conversation",
            Self::Input => "Input",
            Self::Status => "Status",
            Self::PendingAction => "Pending Action",
        }
    }
}

fn render_section(title: &str, body: &str) -> String {
    format!("[{title}]\n{body}\n")
}

#[cfg(test)]
mod tests {
    use elgar_core::{
        action::ActionLifecycleState,
        controller::Controller,
        event::{
            ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
            ErrorEvent, Event, ProviderFinished, ProviderOutput, ProviderStarted, UserMessage,
            VerifiedActionResult,
        },
        session::Session,
    };

    use super::{ActionPanelState, LayoutRegion, TuiShell};

    #[test]
    fn tui_shell_initializes_without_provider_access() {
        let shell = TuiShell::new();

        assert!(shell.conversation.lines.is_empty());
        assert!(shell.input.text.is_empty());
        assert_eq!(shell.status.text, "ready");
        assert_eq!(shell.pending_action.panel, None);
    }

    #[test]
    fn renders_empty_default_state() {
        let rendered = TuiShell::default().render();

        assert!(rendered.contains("[Conversation]\n(empty conversation)"));
        assert!(rendered.contains("[Input]\n> "));
        assert!(rendered.contains("[Status]\nready"));
        assert!(rendered.contains("[Pending Action]\nnone"));
    }

    #[test]
    fn layout_includes_only_the_minimal_chat_first_regions() {
        let shell = TuiShell::new();

        assert_eq!(
            shell.regions(),
            [
                LayoutRegion::Conversation,
                LayoutRegion::PendingAction,
                LayoutRegion::Status,
                LayoutRegion::Input,
            ]
        );
    }

    #[test]
    fn tui_shell_has_no_runtime_behavior() {
        let mut shell = TuiShell::new();

        shell.input.text = "/help".to_string();

        assert_eq!(shell.input.text, "/help");
        assert_eq!(shell.pending_action.panel, None);
        assert!(shell.conversation.lines.is_empty());
    }

    #[test]
    fn consumes_core_events_into_conversation_without_provider_access() {
        let controller = Controller::default();
        let mut session = Session::new("session-1", ".", ".");

        let result = controller.turn(&mut session, "what does the harness do?");
        let mut shell = TuiShell::new();
        shell.consume_events(&result.events);

        let rendered = shell.render();

        assert!(rendered.contains("user: what does the harness do?"));
        assert!(rendered.contains("provider started: stub-provider request stub-request-1"));
        assert!(rendered.contains("provider finished: stub-provider request stub-request-1"));
        assert!(rendered.contains("assistant Provider: stub provider response"));
        assert!(session.actions.is_empty());
    }

    #[test]
    fn conversation_displays_user_assistant_provider_action_and_error_output() {
        let mut shell = TuiShell::new();
        let events = vec![
            Event::UserMessage(UserMessage::new("hello")),
            Event::AssistantMessage(AssistantMessage::new(
                "hi",
                AssistantMessageSource::Controller,
            )),
            Event::ProviderStarted(ProviderStarted::new("stub-provider", "request-1")),
            Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("provider text"),
            )),
            Event::ActionProposed(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )),
            Event::ActionApproved(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )),
            Event::ActionApplied(ActionApplied::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                VerifiedActionResult::FileWritten {
                    path: "hello.py".to_string(),
                },
            )),
            Event::ActionRejected(ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::WriteFile,
                "write rejected.py",
            )),
            Event::ActionFailed(ActionFailed::new(
                "action-3",
                elgar_core::event::ActionKind::WriteFile,
                "permission denied",
            )),
            Event::Error(ErrorEvent::new("boom")),
        ];

        shell.consume_events(&events);

        let rendered = shell.render();
        assert!(rendered.contains("user: hello"));
        assert!(rendered.contains("assistant Controller: hi"));
        assert!(rendered.contains("provider started: stub-provider request request-1"));
        assert!(
            rendered.contains("provider finished: stub-provider request request-1: provider text")
        );
        assert!(rendered.contains("action proposed: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("action approved: action-1 WriteFile write hello.py"));
        assert!(rendered.contains("action applied: action-1 WriteFile file written: hello.py"));
        assert!(rendered.contains("action rejected: action-2 WriteFile write rejected.py"));
        assert!(rendered.contains("action failed: action-3 WriteFile permission denied"));
        assert!(rendered.contains("error: boom"));
        assert!(rendered.contains("[Status]\nerror"));
    }

    #[test]
    fn pending_action_area_shows_proposed_action_from_core_event() {
        let mut shell = TuiShell::new();

        shell.consume_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let panel = shell.pending_action.panel.as_ref().unwrap();
        assert_eq!(panel.action_id, "action-1");
        assert_eq!(panel.action_type, "WriteFile");
        assert_eq!(panel.target.as_deref(), Some("hello.py"));
        assert_eq!(panel.summary, "write hello.py");
        assert_eq!(panel.state, ActionPanelState::Proposed);

        let rendered = shell.render();
        assert!(rendered.contains("[Pending Action]\naction id: action-1"));
        assert!(rendered.contains("action type: WriteFile"));
        assert!(rendered.contains("target: hello.py"));
        assert!(rendered.contains("summary: write hello.py"));
        assert!(rendered.contains("state: pending approval"));
        assert!(rendered.contains("instructions: type approve to apply or reject to decline"));
    }

    #[test]
    fn terminal_action_events_render_result_state() {
        let mut shell = TuiShell::new();

        shell.consume_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));
        shell.consume_event(&Event::ActionRejected(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let rendered = shell.render();
        assert!(rendered.contains("state: rejected"));
        assert!(rendered.contains("result: rejected by user; no filesystem change was made"));
        assert!(rendered.contains(
            "instructions: rejected actions are terminal; submit a new proposal to reconsider"
        ));
    }

    #[test]
    fn rendering_core_events_does_not_mutate_session_files_or_action_truth() {
        let controller = Controller::default();
        let root = std::env::temp_dir().join(format!(
            "elgar-tui-render-{}-no-mutation",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("hello.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());

        controller.turn(&mut session, "create file hello.py");
        let before = session.clone();

        let mut shell = TuiShell::new();
        shell.consume_session(&session);

        assert_eq!(session, before);
        assert!(!target.exists());
        assert_eq!(session.actions.len(), 1);
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions[0].verified_result, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_approval_is_routed_through_controller_and_renders_applied_result() {
        let controller = Controller::default();
        let root = temp_root("approve-through-controller");
        let target = root.join("hello.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(&mut session, "create file hello.py");
        shell.consume_events(&proposed.events);
        assert!(!target.exists());
        assert!(shell.render().contains("state: pending approval"));

        let approved = shell.submit_approval(&controller, &mut session);

        assert_eq!(approved.route, elgar_core::router::Route::ApproveAction);
        assert!(target.exists());
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(matches!(
            session.actions[0].verified_result,
            Some(VerifiedActionResult::FileWritten { .. })
        ));

        let rendered = shell.render();
        assert!(rendered.contains("action id: action-1"));
        assert!(rendered.contains("action type: WriteFile"));
        assert!(rendered.contains("target: hello.py"));
        assert!(rendered.contains("state: applied"));
        assert!(rendered.contains("result: file written:"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_rejection_is_routed_through_controller_and_does_not_write() {
        let controller = Controller::default();
        let root = temp_root("reject-through-controller");
        let target = root.join("hello.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(&mut session, "create file hello.py");
        shell.consume_events(&proposed.events);

        let rejected = shell.submit_rejection(&controller, &mut session);

        assert_eq!(rejected.route, elgar_core::router::Route::RejectAction);
        assert!(!target.exists());
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Rejected
        );
        assert_eq!(session.actions[0].verified_result, None);

        let rendered = shell.render();
        assert!(rendered.contains("state: rejected"));
        assert!(rendered.contains("result: rejected by user; no filesystem change was made"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_renders_failed_result_from_controller_events() {
        let controller = Controller::default();
        let root = temp_root("failed-through-controller");
        let absolute_target = root.join("blocked.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(
            &mut session,
            &format!("create file {}", absolute_target.display()),
        );
        shell.consume_events(&proposed.events);

        shell.submit_approval(&controller, &mut session);

        assert!(!absolute_target.exists());
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Failed
        );

        let rendered = shell.render();
        assert!(rendered.contains("state: failed"));
        assert!(rendered.contains("result: unsafe write target"));

        let _ = std::fs::remove_dir_all(root);
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("elgar-tui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
