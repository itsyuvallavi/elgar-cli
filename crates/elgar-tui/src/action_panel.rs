use elgar_core::event::{ActionEvent, Event, VerifiedActionResult};

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
                Some("Rejected. No file was changed.".to_string()),
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

    pub(crate) fn render_body(&self) -> String {
        self.panel
            .as_ref()
            .map(ActionApprovalPanel::render)
            .unwrap_or_else(|| "none".to_string())
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
                summary: "not available from this result".to_string(),
                state,
                result,
            });
        }
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
            format!("Action: {} {}", self.action_id, self.action_type),
            format!(
                "Target: {}",
                self.target.as_deref().unwrap_or("unavailable")
            ),
            format!("Summary: {}", self.summary),
            format!("State: {}", self.state.render()),
        ];

        if let Some(result) = &self.result {
            lines.push(format!("Result: {result}"));
        }

        lines.push(self.state.instructions().to_string());
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
            Self::Proposed => "waiting for approval",
            Self::Approved => "approved",
            Self::Applied => "applied and verified",
            Self::Rejected => "rejected",
            Self::Failed => "failed",
        }
    }

    fn instructions(self) -> &'static str {
        match self {
            Self::Proposed => "Approve to apply, or reject to leave things unchanged.",
            Self::Approved => "Approval recorded. Applying through the controller.",
            Self::Applied => "Verified by the controller.",
            Self::Rejected => "Rejected actions are final. Start a new proposal to reconsider.",
            Self::Failed => "Failure recorded by the controller.",
        }
    }
}

fn render_verified_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("file written: {path}")
        }
    }
}

#[cfg(test)]
mod tests {
    use elgar_core::event::{ActionApplied, ActionEvent, Event, VerifiedActionResult};

    use super::{ActionPanelState, PendingActionArea};

    #[test]
    fn pending_action_area_shows_proposed_action_from_core_event() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let panel = pending_action.panel.as_ref().unwrap();
        assert_eq!(panel.action_id, "action-1");
        assert_eq!(panel.action_type, "WriteFile");
        assert_eq!(panel.target.as_deref(), Some("hello.py"));
        assert_eq!(panel.summary, "write hello.py");
        assert_eq!(panel.state, ActionPanelState::Proposed);

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Action: action-1 WriteFile"));
        assert!(rendered.contains("Target: hello.py"));
        assert!(rendered.contains("Summary: write hello.py"));
        assert!(rendered.contains("State: waiting for approval"));
        assert!(rendered.contains("Approve to apply, or reject to leave things unchanged."));
    }

    #[test]
    fn terminal_action_events_render_result_state() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionProposed(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));
        pending_action.observe_event(&Event::ActionRejected(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::WriteFile,
                "write hello.py",
            )
            .with_target("hello.py"),
        ));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("State: rejected"));
        assert!(rendered.contains("Result: Rejected. No file was changed."));
        assert!(
            rendered.contains("Rejected actions are final. Start a new proposal to reconsider.")
        );
    }

    #[test]
    fn applied_result_without_current_panel_uses_fallback_details() {
        let mut pending_action = PendingActionArea::default();

        pending_action.observe_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::WriteFile,
            VerifiedActionResult::FileWritten {
                path: "hello.py".to_string(),
            },
        )));

        let rendered = pending_action.render_body();
        assert!(rendered.contains("Action: action-1 unknown"));
        assert!(rendered.contains("Result: file written: hello.py"));
    }
}
