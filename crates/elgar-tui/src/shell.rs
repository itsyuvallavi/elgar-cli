use elgar_core::{
    action_gate::ActionGate, agent_runtime::AgentRuntime, controller::TurnResult, event::Event,
    policy::PermissionPolicyMode, provider::ControllerProvider, session::Session,
};

use crate::{
    action_panel::PendingActionArea,
    layout::{render_section, LayoutRegion},
    panes::{ConversationPane, CopyArea, InputArea, StatusLine},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TuiShell {
    pub conversation: ConversationPane,
    pub input: InputArea,
    pub status: StatusLine,
    pub pending_action: PendingActionArea,
    pub copy: CopyArea,
    pub policy_mode: PermissionPolicyMode,
}

impl TuiShell {
    pub fn new() -> Self {
        Self::with_policy_mode(PermissionPolicyMode::AutoCreateReviewModify)
    }

    pub fn with_policy_mode(policy_mode: PermissionPolicyMode) -> Self {
        Self {
            conversation: ConversationPane::default(),
            input: InputArea::default(),
            status: StatusLine::ready(),
            pending_action: PendingActionArea::default(),
            copy: CopyArea::default(),
            policy_mode,
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
        self.consume_events(session.events());
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

    pub fn submit_approval<P>(
        &mut self,
        action_gate: &ActionGate<P>,
        session: &mut Session,
    ) -> TurnResult
    where
        P: ControllerProvider,
    {
        let result = action_gate.approve(session);
        self.consume_events(&result.events);
        self.conversation.follow_latest();
        result
    }

    pub fn submit_rejection<P>(
        &mut self,
        action_gate: &ActionGate<P>,
        session: &mut Session,
    ) -> TurnResult
    where
        P: ControllerProvider,
    {
        let result = action_gate.reject(session);
        self.consume_events(&result.events);
        self.conversation.follow_latest();
        result
    }

    pub fn submit_agent_input<P>(
        &mut self,
        runtime: &AgentRuntime<P>,
        session: &mut Session,
        input: &str,
    ) -> TurnResult
    where
        P: ControllerProvider,
    {
        let result = runtime.turn(session, input, self.policy_mode);
        self.consume_events(&result.events);
        self.conversation.follow_latest();
        result
    }

    pub fn conversation_copy_text(&self) -> String {
        self.conversation.render_copy_body()
    }

    pub fn clear_conversation(&mut self) {
        self.conversation.lines.clear();
        self.conversation.follow_latest();
    }

    pub fn apply_permission_command(&mut self, argument: Option<&str>) -> String {
        let Some(argument) = argument.map(str::trim).filter(|value| !value.is_empty()) else {
            return render_permission_mode_status(self.policy_mode);
        };

        if argument.eq_ignore_ascii_case("next") {
            self.policy_mode = self.policy_mode.next();
            return format!(
                "Permission mode set to {}: {}.",
                self.policy_mode,
                self.policy_mode.description()
            );
        }

        match PermissionPolicyMode::parse(argument) {
            Ok(mode) => {
                self.policy_mode = mode;
                format!(
                    "Permission mode set to {}: {}.",
                    self.policy_mode,
                    self.policy_mode.description()
                )
            }
            Err(_) => format!(
                "Unknown permission mode `{argument}`. Use one of: {}.",
                permission_mode_names()
            ),
        }
    }

    pub fn push_local_message(&mut self, message: impl Into<String>) {
        self.conversation.push_local_message(message);
        self.conversation.follow_latest();
    }
}

fn render_permission_mode_status(mode: PermissionPolicyMode) -> String {
    format!(
        "Permission mode is {}: {}.\nAvailable modes: {}.",
        mode,
        mode.description(),
        permission_mode_names()
    )
}

fn permission_mode_names() -> String {
    PermissionPolicyMode::ALL
        .iter()
        .map(|mode| mode.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

impl Default for TuiShell {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::{
        action::ActionLifecycleState, action_gate::ActionGate, agent_runtime::AgentRuntime,
        controller::Controller, event::VerifiedActionResult, session::Session,
    };

    use crate::layout::LayoutRegion;

    use super::TuiShell;

    #[test]
    fn tui_shell_initializes_without_provider_access() {
        let shell = TuiShell::new();

        assert!(shell.conversation.lines.is_empty());
        assert!(shell.input.text.is_empty());
        assert_eq!(shell.status.text, "ready");
        assert_eq!(shell.pending_action.panel, None);
        assert_eq!(shell.copy.render_hint(), "");
        assert_eq!(
            shell.policy_mode,
            elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify
        );
    }

    #[test]
    fn permission_command_shows_sets_and_cycles_modes() {
        let mut shell =
            TuiShell::with_policy_mode(elgar_core::policy::PermissionPolicyMode::ReviewAll);

        let status = shell.apply_permission_command(None);
        assert!(status.contains("Permission mode is review_all"));

        let cycled = shell.apply_permission_command(Some("next"));
        assert!(cycled.contains("auto_create_review_modify"));
        assert_eq!(
            shell.policy_mode,
            elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify
        );

        let set = shell.apply_permission_command(Some("full-access"));
        assert!(set.contains("full_access"));
        assert_eq!(
            shell.policy_mode,
            elgar_core::policy::PermissionPolicyMode::FullAccess
        );

        let invalid = shell.apply_permission_command(Some("anything"));
        assert!(invalid.contains("Unknown permission mode"));
        assert_eq!(
            shell.policy_mode,
            elgar_core::policy::PermissionPolicyMode::FullAccess
        );
    }

    #[test]
    fn renders_empty_default_state() {
        let rendered = TuiShell::default().render();

        assert!(rendered.contains("Conversation\n(empty conversation)"));
        assert!(rendered.contains("Input\n> "));
        assert!(rendered.contains("Status\nready"));
        assert!(rendered.contains("Pending Action\nnone"));
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

        assert!(rendered.contains("> what does the harness do?"));
        assert!(!rendered.contains("User\n"));
        assert!(!rendered.contains("thinking"));
        assert!(rendered.contains("stub provider response"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("stub-request-1"));
        assert!(!rendered.contains("Provider text is suggestion only."));
        assert!(session.actions().is_empty());
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
        assert_eq!(session.actions().len(), 1);
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions()[0].verified_result, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn submitting_input_follows_latest_without_mutating_session_truth() {
        let mut session = Session::new("session-1", ".", ".");
        let before = session.clone();
        let mut shell = TuiShell::new();
        shell.conversation.lines = (0..10).map(|index| format!("line {index}")).collect();
        shell.conversation.scroll_up(5);

        let runtime = AgentRuntime::default();

        let result = shell.submit_agent_input(&runtime, &mut session, "what does the harness do?");

        assert_eq!(result.route, elgar_core::router::Route::AskModel);
        assert_eq!(shell.conversation.scroll_offset(4), 8);
        assert_eq!(before.events().len(), 0);
        assert_eq!(session.events().len(), result.events.len());
    }

    #[test]
    fn tui_approval_is_routed_through_action_gate_and_renders_applied_result() {
        let controller = Controller::default();
        let action_gate = ActionGate::default();
        let root = temp_root("approve-through-controller");
        let target = root.join("hello.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(&mut session, "create file hello.py");
        shell.consume_events(&proposed.events);
        assert!(!target.exists());
        assert!(shell.render().contains("Status: waiting for approval"));

        let approved = shell.submit_approval(&action_gate, &mut session);

        assert_eq!(approved.route, elgar_core::router::Route::ApproveAction);
        assert!(target.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Applied
        );
        assert!(matches!(
            session.actions()[0].verified_result,
            Some(VerifiedActionResult::FileWritten { .. })
        ));

        let rendered = shell.render();
        assert!(rendered.contains("File: hello.py"));
        assert!(rendered.contains("Status: applied and verified"));
        assert!(rendered.contains("Result: Wrote "));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_rejection_is_routed_through_action_gate_and_does_not_write() {
        let controller = Controller::default();
        let action_gate = ActionGate::default();
        let root = temp_root("reject-through-controller");
        let target = root.join("hello.py");
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(&mut session, "create file hello.py");
        shell.consume_events(&proposed.events);

        let rejected = shell.submit_rejection(&action_gate, &mut session);

        assert_eq!(rejected.route, elgar_core::router::Route::RejectAction);
        assert!(!target.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Rejected
        );
        assert_eq!(session.actions()[0].verified_result, None);

        let rendered = shell.render();
        assert!(rendered.contains("Status: rejected"));
        assert!(rendered.contains("Result: Rejected. No file was changed."));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn tui_renders_failed_result_from_controller_events() {
        let controller = Controller::default();
        let action_gate = ActionGate::default();
        let root = temp_root("failed-through-controller");
        let absolute_target = std::env::temp_dir().join(format!(
            "elgar-tui-{}-blocked-outside-root.py",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&absolute_target);
        let mut session = Session::new("session-1", root.clone(), root.clone());
        let mut shell = TuiShell::new();

        let proposed = controller.turn(
            &mut session,
            &format!("create file {}", absolute_target.display()),
        );
        shell.consume_events(&proposed.events);

        shell.submit_approval(&action_gate, &mut session);

        assert!(!absolute_target.exists());
        assert_eq!(
            session.actions()[0].action.state,
            ActionLifecycleState::Failed
        );

        let rendered = shell.render();
        assert!(rendered.contains("Status: failed"));
        assert!(rendered.contains("Result: unsafe write target"));

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(absolute_target);
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("elgar-tui-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }
}
