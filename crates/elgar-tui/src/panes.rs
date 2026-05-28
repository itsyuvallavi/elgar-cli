use std::path::{Path, PathBuf};

use elgar_core::event::{
    ActionEvent, ActionKind, AssistantMessageSource, Event, FileActionVerification,
    VerifiedActionResult,
};
use elgar_core::policy::ApprovalSource;

use crate::markdown::render_assistant_markdown;
use crate::shell_result::render_shell_execution_details;

mod provider_thinking;
mod tool_activity;

#[cfg(test)]
use provider_thinking::is_low_value_provider_tool_planning_thinking;
use provider_thinking::render_provider_thinking;
use tool_activity::{create_write_tool_item, CreateWriteToolBatch, CreateWriteToolItem};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConversationPane {
    pub lines: Vec<String>,
    line_styles: Vec<ConversationLineStyle>,
    scrollback: ConversationScrollback,
    loading_pulse: ThinkingPulse,
    create_batch: Option<CreateWriteToolBatch>,
}

impl ConversationPane {
    pub fn push_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(_) => self.loading_pulse.reset(),
            Event::ProviderFinished(_) | Event::Error(_) => self.remove_loading_pulse(),
            _ => {}
        }

        if let Event::ActionApplied(applied) = event {
            if let Some(item) = create_write_tool_item(&applied.result) {
                self.push_create_batch_item(item);
                return;
            }
        }

        if is_hidden_policy_approval(event) {
            return;
        }

        if !matches!(event, Event::Error(_)) {
            self.create_batch = None;
        }

        if let Some((line, style)) = render_tui_event(event) {
            self.push_line(line, style);
        }
    }

    #[cfg(test)]
    pub(crate) fn scroll_up(&mut self, lines: usize) {
        self.scrollback.scroll_up(lines);
    }

    #[cfg(test)]
    pub(crate) fn scroll_down(&mut self, lines: usize) {
        self.scrollback.scroll_down(lines);
    }

    pub(crate) fn follow_latest(&mut self) {
        self.scrollback.follow_latest();
    }

    #[cfg(test)]
    pub(crate) fn advance_loading_pulse(&mut self) {
        if self.last_line_style() == Some(ConversationLineStyle::Loading) {
            self.loading_pulse.advance();
            if let Some(last_line) = self.lines.last_mut() {
                *last_line = self.loading_pulse.label().to_string();
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_pending_provider_turn(&mut self, content: &str) {
        self.loading_pulse.reset();
        self.push_line(render_user_message(content), ConversationLineStyle::User);
        self.push_line(
            self.loading_pulse.label().to_string(),
            ConversationLineStyle::Loading,
        );
    }

    #[cfg(test)]
    pub(crate) fn discard_pending_provider_turn(&mut self) {
        self.remove_loading_pulse();
        if self.last_line_style() == Some(ConversationLineStyle::User) {
            self.pop_line();
        }
    }

    fn remove_loading_pulse(&mut self) {
        if self.last_line_style() == Some(ConversationLineStyle::Loading) {
            self.pop_line();
        }
    }

    pub fn push_local_message(&mut self, message: impl Into<String>) {
        self.push_line(message.into(), ConversationLineStyle::Plain);
    }

    #[cfg(test)]
    pub(crate) fn scroll_offset(&self, viewport_height: u16) -> u16 {
        self.scroll_offset_for_lines(self.render_line_count(), viewport_height)
    }

    pub(crate) fn scroll_offset_for_lines(
        &self,
        content_lines: usize,
        viewport_height: u16,
    ) -> u16 {
        self.scrollback
            .offset_for(content_lines, usize::from(viewport_height))
    }

    #[cfg(test)]
    fn render_line_count(&self) -> usize {
        self.render_body().lines().count().max(1)
    }

    pub(crate) fn render_body(&self) -> String {
        if self.lines.is_empty() {
            "(empty conversation)".to_string()
        } else {
            self.lines.join("\n")
        }
    }

    pub(crate) fn render_copy_body(&self) -> String {
        let lines = self
            .lines
            .iter()
            .enumerate()
            .filter(|(index, _line)| self.line_style(*index) != ConversationLineStyle::Thinking)
            .map(|(_index, line)| line.as_str())
            .collect::<Vec<_>>();

        if lines.is_empty() {
            "(empty conversation)".to_string()
        } else {
            lines.join("\n")
        }
    }

    pub(crate) fn render_lines_with_styles(&self) -> Vec<(String, ConversationLineStyle)> {
        if self.lines.is_empty() {
            return vec![(
                "(empty conversation)".to_string(),
                ConversationLineStyle::Plain,
            )];
        }

        self.lines
            .iter()
            .enumerate()
            .flat_map(|(index, entry)| {
                let style = self.line_style(index);
                entry
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(move |line| (line.to_string(), style))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn push_line(&mut self, line: String, style: ConversationLineStyle) {
        self.align_line_styles();
        self.lines.push(line);
        self.line_styles.push(style);
    }

    fn pop_line(&mut self) -> Option<String> {
        let line = self.lines.pop();
        if self.line_styles.len() > self.lines.len() {
            self.line_styles.pop();
        }
        line
    }

    fn last_line_style(&self) -> Option<ConversationLineStyle> {
        self.lines
            .len()
            .checked_sub(1)
            .map(|index| self.line_style(index))
    }

    fn line_style(&self, index: usize) -> ConversationLineStyle {
        self.line_styles
            .get(index)
            .copied()
            .unwrap_or(ConversationLineStyle::Plain)
    }

    fn align_line_styles(&mut self) {
        while self.line_styles.len() < self.lines.len() {
            self.line_styles.push(ConversationLineStyle::Plain);
        }
    }

    fn push_create_batch_item(&mut self, item: CreateWriteToolItem) {
        match &mut self.create_batch {
            Some(batch) => {
                batch.push(item);
                if let Some(line) = self.lines.get_mut(batch.line_index) {
                    *line = batch.render();
                }
                if let Some(style) = self.line_styles.get_mut(batch.line_index) {
                    *style = batch.line_style();
                }
            }
            None => {
                let line_index = self.lines.len();
                let batch = CreateWriteToolBatch::new(line_index, item);
                let line = batch.render();
                let style = batch.line_style();
                self.create_batch = Some(batch);
                self.push_line(line, style);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ConversationLineStyle {
    #[default]
    Plain,
    User,
    Loading,
    Thinking,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct ConversationScrollback {
    lines_from_bottom: usize,
}

impl ConversationScrollback {
    #[cfg(test)]
    fn scroll_up(&mut self, lines: usize) {
        self.lines_from_bottom = self.lines_from_bottom.saturating_add(lines);
    }

    #[cfg(test)]
    fn scroll_down(&mut self, lines: usize) {
        self.lines_from_bottom = self.lines_from_bottom.saturating_sub(lines);
    }

    fn follow_latest(&mut self) {
        self.lines_from_bottom = 0;
    }

    fn offset_for(&self, content_lines: usize, viewport_lines: usize) -> u16 {
        let max_offset = content_lines.saturating_sub(viewport_lines.max(1));
        max_offset
            .saturating_sub(self.lines_from_bottom.min(max_offset))
            .min(usize::from(u16::MAX)) as u16
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputArea {
    pub text: String,
}

impl InputArea {
    pub(crate) fn render_body(&self) -> String {
        format!("> {}", self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyArea {
    last_result: Option<CopyResult>,
}

impl CopyArea {
    pub(crate) fn mark_copied(&mut self, bytes: usize) {
        self.last_result = Some(CopyResult::Copied { bytes });
    }

    pub(crate) fn mark_failed(&mut self, message: impl Into<String>) {
        self.last_result = Some(CopyResult::Failed {
            message: message.into(),
        });
    }

    pub(crate) fn render_hint(&self) -> String {
        match &self.last_result {
            Some(CopyResult::Copied { bytes }) => {
                format!("copied conversation ({bytes} bytes)")
            }
            Some(CopyResult::Failed { message }) => {
                format!("copy failed: {message}")
            }
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CopyResult {
    Copied { bytes: usize },
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    thinking_pulse: ThinkingPulse,
    provider_active: bool,
}

impl StatusLine {
    pub fn ready() -> Self {
        Self {
            text: "ready".to_string(),
            thinking_pulse: ThinkingPulse::default(),
            provider_active: false,
        }
    }

    pub fn observe_event(&mut self, event: &Event) {
        match event {
            Event::ProviderStarted(_) => self.start_thinking_pulse(),
            Event::ProviderFinished(_) => self.finish("reply ready"),
            Event::Error(error) => {
                if parse_provider_error(&error.message).is_some() {
                    self.finish("provider error");
                } else {
                    self.finish("error");
                }
            }
            _ => {
                self.provider_active = false;
                self.text = match event {
                    Event::UserMessage(_) => "sent".to_string(),
                    Event::AssistantMessage(_) => "reply ready".to_string(),
                    Event::ActionProposed(action) => {
                        format!("review {}", action.action_id)
                    }
                    Event::ActionApproved(action) => {
                        format!("approved {}", action.action_id)
                    }
                    Event::ActionRejected(action) => {
                        format!("rejected {}", action.action_id)
                    }
                    Event::ActionApplied(action) => {
                        format!("applied {}", action.action_id)
                    }
                    Event::ActionFailed(action) => {
                        format!("failed {}", action.action_id)
                    }
                    Event::ProviderStarted(_) | Event::ProviderFinished(_) | Event::Error(_) => {
                        unreachable!("provider and error events are handled above")
                    }
                };
            }
        }
    }

    pub(crate) fn start_thinking_pulse(&mut self) {
        self.provider_active = true;
        self.thinking_pulse.reset();
        self.text = self.thinking_pulse.label().to_string();
    }

    #[cfg(test)]
    pub(crate) fn cancel_provider_turn(&mut self) {
        self.finish("canceled");
    }

    #[cfg(test)]
    pub(crate) fn advance_thinking_pulse(&mut self) {
        if self.provider_active {
            self.thinking_pulse.advance();
            self.text = self.thinking_pulse.label().to_string();
        }
    }

    #[cfg(test)]
    pub(crate) fn provider_active(&self) -> bool {
        self.provider_active
    }

    pub(crate) fn render_body(&self) -> String {
        self.text.clone()
    }

    fn finish(&mut self, text: &'static str) {
        self.provider_active = false;
        self.text = text.to_string();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ThinkingPulse {
    index: usize,
}

impl ThinkingPulse {
    const LABELS: [&'static str; 4] = ["◐ working", "◓ working", "◑ working", "◒ working"];

    fn label(&self) -> &'static str {
        Self::LABELS[self.index]
    }

    #[cfg(test)]
    fn advance(&mut self) {
        self.index = (self.index + 1) % Self::LABELS.len();
    }

    fn reset(&mut self) {
        self.index = 0;
    }
}

fn render_tui_event(event: &Event) -> Option<(String, ConversationLineStyle)> {
    match event {
        Event::UserMessage(message) => Some((
            render_user_message(&message.content),
            ConversationLineStyle::User,
        )),
        Event::AssistantMessage(message) => {
            if message.source == AssistantMessageSource::Controller
                && is_controller_action_boilerplate(&message.content)
            {
                return None;
            }

            let rendered = match message.source {
                AssistantMessageSource::Controller | AssistantMessageSource::Provider => {
                    render_assistant_output(&message.content)
                }
            };
            Some((rendered, ConversationLineStyle::Plain))
        }
        Event::ProviderStarted(_) => {
            Some((render_thinking_progress(), ConversationLineStyle::Loading))
        }
        Event::ProviderFinished(finished) => {
            if !finished.output.tool_calls.is_empty() {
                return None;
            }

            render_provider_thinking(finished.output.thinking.as_deref())
                .map(|line| (line, ConversationLineStyle::Thinking))
        }
        Event::ActionProposed(action) => {
            Some((render_action_proposed(action), ConversationLineStyle::Plain))
        }
        Event::ActionApproved(action) => {
            render_action_approved(action).map(|line| (line, ConversationLineStyle::Plain))
        }
        Event::ActionRejected(action) => {
            Some((render_action_rejected(action), ConversationLineStyle::Plain))
        }
        Event::ActionApplied(applied) => Some((
            render_verified_action_result(&applied.result),
            ConversationLineStyle::Plain,
        )),
        Event::ActionFailed(failed) => Some(format!(
            "Action failed: {} {:?} {}",
            failed.action_id, failed.action_kind, failed.reason
        ))
        .map(|line| (line, ConversationLineStyle::Plain)),
        Event::Error(error) => Some((
            render_error_line(&error.message),
            ConversationLineStyle::Plain,
        )),
    }
}

fn is_hidden_policy_approval(event: &Event) -> bool {
    matches!(
        event,
        Event::ActionApproved(action)
            if action
                .approval_source
                .as_ref()
                .is_some_and(ApprovalSource::is_policy)
    )
}

fn render_action_proposed(action: &ActionEvent) -> String {
    if let Some(path) = create_directory_summary_path(&action.summary) {
        return format!(
            "I can create {}. Approve to create it.",
            user_display_path(path)
        );
    }

    match action.action_kind {
        ActionKind::CreateFile => format!(
            "I can write {}. Approve to write it.",
            action
                .target
                .as_deref()
                .or_else(|| action.summary.strip_prefix("write ").map(str::trim))
                .unwrap_or(&action.summary)
        ),
        ActionKind::CreateDirectory => format!(
            "I can create {}. Approve to create it.",
            action.target.as_deref().unwrap_or(&action.summary)
        ),
        ActionKind::ShellCommand => render_shell_action_proposal(action),
        _ => format!(
            "I can apply this action: {}. Approve to continue.",
            action.summary
        ),
    }
}

fn render_action_approved(action: &ActionEvent) -> Option<String> {
    if action
        .approval_source
        .as_ref()
        .is_some_and(ApprovalSource::is_policy)
    {
        return render_policy_approved_action(action);
    }

    if let Some(path) = create_directory_summary_path(&action.summary) {
        return Some(format!("Approved. Creating {}.", user_display_path(path)));
    }

    if action.summary.starts_with("create Markdown plan ") {
        return Some("Approved. Creating the plan.".to_string());
    }

    if action.summary.starts_with("execute Markdown plan in ") {
        return Some("Approved. Creating the project files.".to_string());
    }

    Some("Approved. Applying the action.".to_string())
}

fn render_policy_approved_action(_action: &ActionEvent) -> Option<String> {
    None
}

fn render_action_rejected(action: &ActionEvent) -> String {
    if let Some(path) = create_directory_summary_path(&action.summary) {
        return format!("Rejected. Did not create {}.", user_display_path(path));
    }

    "Rejected. No changes were made.".to_string()
}

fn create_directory_summary_path(summary: &str) -> Option<&str> {
    summary
        .strip_prefix("create directory ")
        .map(str::trim)
        .filter(|path| !path.is_empty())
}

fn render_user_message(content: &str) -> String {
    content
        .lines()
        .map(|line| format!("> {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_assistant_output(content: &str) -> String {
    render_assistant_markdown(content)
}

fn render_shell_action_proposal(action: &ActionEvent) -> String {
    if let Some(path) = action.summary.strip_prefix("create Markdown plan ") {
        return format!(
            "I can create the plan at {}. Approve to write it.",
            user_display_path(path.trim())
        );
    }

    if let Some(path) = action.summary.strip_prefix("execute Markdown plan in ") {
        return format!(
            "I can create the project files in {}. Approve to create them.",
            user_display_path(path.trim())
        );
    }

    "I can run this command. Approve to run it.".to_string()
}

fn is_controller_action_boilerplate(content: &str) -> bool {
    let trimmed = content.trim();

    if trimmed.starts_with("Proposed ") && trimmed.contains(" action") {
        return true;
    }

    if trimmed.starts_with("Model-first tool call validated") {
        return true;
    }

    if trimmed.starts_with("I can create ")
        || trimmed.starts_with("I can write ")
        || trimmed.starts_with("I can apply this action:")
        || trimmed == "I can run the shell command. Approve to run it."
    {
        return true;
    }

    if trimmed.starts_with("Approved ") && trimmed.contains("Applying through the controller") {
        return true;
    }

    if matches!(
        trimmed,
        "Executed approved shell command and recorded the verified result."
            | "Applied approved action and recorded the verified result."
    ) {
        return true;
    }

    [
        "Created ",
        "Wrote ",
        "Updated ",
        "Overwrote ",
        "Deleted ",
        "Moved ",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
}

fn render_thinking_progress() -> String {
    ThinkingPulse::default().label().to_string()
}

fn render_verified_action_result(result: &VerifiedActionResult) -> String {
    match result {
        VerifiedActionResult::FileWritten { path } => {
            format!("Wrote {}.", user_display_path(path))
        }
        VerifiedActionResult::File(file) => render_file_verification(file),
        VerifiedActionResult::Shell(shell) => {
            if let Some(effect) = &shell.verified_effect {
                if let Some(message) = render_shell_verified_effect(effect) {
                    return message;
                }
            }
            render_shell_execution_details(shell)
        }
    }
}

fn render_shell_verified_effect(effect: &str) -> Option<String> {
    if let Some(path) = verified_effect_value(effect, "verified file exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified files exist: ") {
        return Some(format!("Created files: {}.", user_display_path_list(paths)));
    }

    if let Some(path) = verified_effect_value(effect, "verified directory exists: ") {
        return Some(format!("Created {}.", user_display_path(path)));
    }

    if let Some(paths) = verified_effect_value(effect, "verified directories exist: ") {
        return Some(format!("Created {}.", user_display_path_list(paths)));
    }

    None
}

fn verified_effect_value<'a>(effect: &'a str, prefix: &str) -> Option<&'a str> {
    effect
        .split("; ")
        .find_map(|part| part.strip_prefix(prefix).map(str::trim))
        .filter(|value| !value.is_empty())
}

fn render_file_verification(result: &FileActionVerification) -> String {
    match result {
        FileActionVerification::FileCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
        FileActionVerification::FilePatched { path } => {
            format!("Updated {}.", user_display_path(path))
        }
        FileActionVerification::FileOverwritten { path } => {
            format!("Overwrote {}.", user_display_path(path))
        }
        FileActionVerification::FileDeleted { path } => {
            format!("Deleted {}.", user_display_path(path))
        }
        FileActionVerification::FileMoved {
            source_path,
            target_path,
        } => format!(
            "Moved {} to {}.",
            user_display_path(source_path),
            user_display_path(target_path)
        ),
        FileActionVerification::DirectoryCreated { path } => {
            format!("Created {}.", user_display_path(path))
        }
    }
}

fn user_display_path(path: impl AsRef<Path>) -> String {
    let path = path.as_ref();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
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

fn user_display_path_list(paths: &str) -> String {
    paths
        .split(", ")
        .map(user_display_path)
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_error_line(message: &str) -> String {
    if let Some(provider_error) = parse_provider_error(message) {
        format!(
            "Provider error from {}: {}",
            provider_error.provider, provider_error.detail
        )
    } else if let Some(tool_error) = render_model_tool_error(message) {
        tool_error
    } else {
        format!("Error: {message}")
    }
}

fn render_model_tool_error(message: &str) -> Option<String> {
    let rest = message.strip_prefix("model tool `")?;
    let (tool, rest) = rest.split_once('`')?;
    let arg = rest
        .split_once("missing required argument `")
        .and_then(|(_prefix, rest)| rest.split_once('`').map(|(arg, _suffix)| arg));

    match arg {
        Some(arg) => Some(format!(
            "Tool call incomplete: {tool} needs {arg}. No action was applied."
        )),
        None => Some(format!(
            "Tool call malformed: {tool}. No action was applied."
        )),
    }
}

struct ProviderErrorParts<'a> {
    provider: &'a str,
    detail: &'a str,
}

fn parse_provider_error(message: &str) -> Option<ProviderErrorParts<'_>> {
    let (provider, rest) = message.split_once(" provider request ")?;
    let (_request_id, detail) = rest.split_once(" failed: ")?;
    Some(ProviderErrorParts { provider, detail })
}

#[cfg(test)]
mod tests {
    use elgar_core::{
        event::{
            ActionApplied, ActionEvent, ActionFailed, AssistantMessage, AssistantMessageSource,
            ErrorEvent, Event, FileActionVerification, ProviderFinished, ProviderOutput,
            ProviderStarted, ShellActionVerification, UserMessage, VerifiedActionResult,
        },
        model_runtime::{ModelToolName, RawModelToolCall, RawModelToolName},
    };

    use super::{
        is_low_value_provider_tool_planning_thinking, ConversationLineStyle, ConversationPane,
        CopyArea, InputArea, StatusLine,
    };

    #[test]
    fn conversation_displays_user_assistant_provider_action_and_error_output() {
        let mut conversation = ConversationPane::default();
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
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )),
            Event::ActionApproved(ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                "write hello.py",
            )),
            Event::ActionApplied(ActionApplied::new(
                "action-1",
                elgar_core::event::ActionKind::CreateFile,
                VerifiedActionResult::FileWritten {
                    path: "hello.py".to_string(),
                },
            )),
            Event::ActionRejected(ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::CreateFile,
                "write rejected.py",
            )),
            Event::ActionFailed(ActionFailed::new(
                "action-3",
                elgar_core::event::ActionKind::CreateFile,
                "permission denied",
            )),
            Event::Error(ErrorEvent::new("boom")),
        ];

        for event in &events {
            conversation.push_event(event);
        }

        let rendered = conversation.render_body();
        assert!(rendered.contains("> hello"));
        assert!(!rendered.contains("User\n"));
        assert!(rendered.contains("hi"));
        assert!(!rendered.contains("Elgar: hi"));
        assert!(!rendered.contains("thinking"));
        assert!(!rendered.contains("request-1"));
        assert!(!rendered.contains("Provider text is suggestion only."));
        assert!(rendered.contains("I can write hello.py. Approve to write it."));
        assert!(rendered.contains("Approved. Applying the action."));
        assert!(rendered.contains("Wrote hello.py."));
        assert!(rendered.contains("Rejected. No changes were made."));
        assert!(rendered.contains("Action failed: action-3 CreateFile permission denied"));
        assert!(rendered.contains("Error: boom"));
    }

    #[test]
    fn conversation_renders_shell_result_exit_code_and_output() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::ShellCommand,
            VerifiedActionResult::Shell(ShellActionVerification {
                command: "printf hello".to_string(),
                cwd: "/repo".to_string(),
                stdout: "hello\n".to_string(),
                stderr: String::new(),
                stdout_truncated: false,
                stderr_truncated: false,
                exit_code: Some(0),
                elapsed_millis: 12,
                timed_out: false,
                verified_effect: None,
            }),
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("Shell command finished: exit 0."));
        assert!(rendered.contains("stdout: hello"));
        assert!(!rendered.contains("Shell command finished and verification was recorded."));
    }

    #[test]
    fn empty_panes_render_default_body_text() {
        assert_eq!(
            ConversationPane::default().render_body(),
            "(empty conversation)"
        );
        assert_eq!(InputArea::default().render_body(), "> ");
        assert_eq!(CopyArea::default().render_hint(), "");
    }

    #[test]
    fn completed_provider_output_does_not_render_blank_rows() {
        let mut conversation = ConversationPane::default();
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Plan\n\n- One\n\n- Two\n\ncode:\n```python\nprint(\"one\")\n\nprint(\"two\")\n```",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_lines_with_styles();

        assert!(rendered
            .iter()
            .all(|(line, _style)| !line.trim().is_empty()));
        assert_eq!(
            rendered
                .iter()
                .map(|(line, _style)| line.as_str())
                .collect::<Vec<_>>(),
            vec![
                "Plan",
                "- One",
                "- Two",
                "code:",
                "code (python):",
                "    print(\"one\")",
                "    print(\"two\")",
            ]
        );
    }

    #[test]
    fn copy_area_tracks_copy_result_without_changing_conversation() {
        let mut copy = CopyArea::default();

        copy.mark_copied(12);
        assert_eq!(copy.render_hint(), "copied conversation (12 bytes)");

        copy.mark_failed("terminal rejected OSC 52");
        assert_eq!(copy.render_hint(), "copy failed: terminal rejected OSC 52");
    }

    #[test]
    fn status_line_tracks_last_event_kind() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new("boom")));

        assert_eq!(status.text, "error");
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn conversation_renders_provider_errors_with_calm_copy() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));

        assert_eq!(
            conversation.render_body(),
            "Provider error from fake-provider: Provider provider error (404): model missing"
        );
    }

    #[test]
    fn conversation_renders_controller_errors_without_provider_label() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));

        assert_eq!(
            conversation.render_body(),
            "Error: Input was not recognized."
        );
    }

    #[test]
    fn conversation_renders_assistant_markdown_as_presentation_only_text() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Plan:\n- **read** files\n- `render` output\n\n```rust\nfn main() {}\n```",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("Plan:\n- read files\n- render output"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("code (rust):\n    fn main() {}"));
        assert!(!rendered.contains("```"));
        assert!(!rendered.contains("**read**"));
    }

    #[test]
    fn conversation_renders_assistant_markdown_tables_readably() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "| File | State |\n| --- | --- |\n| src/lib.rs | changed |",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        assert!(rendered.contains("  File"));
        assert!(!rendered.contains("Model:"));
        assert!(rendered.contains("src/lib.rs"));
        assert!(rendered.contains("changed"));
        assert!(!rendered.contains("| --- |"));
    }

    #[test]
    fn conversation_uses_pi_style_user_block_and_unlabeled_provider_reply() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::UserMessage(UserMessage::new(
            "explain this\nin two lines",
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "short answer",
            AssistantMessageSource::Provider,
        )));

        assert_eq!(
            conversation.render_body(),
            "> explain this\n> in two lines\nshort answer"
        );
    }

    #[test]
    fn conversation_pulses_loading_inside_transcript() {
        let mut conversation = ConversationPane::default();

        conversation.push_pending_provider_turn("hello");
        assert_eq!(conversation.render_body(), "> hello\n◐ working");

        conversation.advance_loading_pulse();
        assert_eq!(conversation.render_body(), "> hello\n◓ working");

        conversation.discard_pending_provider_turn();
        assert_eq!(conversation.render_body(), "(empty conversation)");
    }

    #[test]
    fn conversation_renders_explicit_provider_thinking_before_model_answer() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer")
                .with_thinking("Need to respond as Elgar, short.\nSimple greeting."),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();
        let thinking_index = rendered.find("Need to respond as Elgar, short.").unwrap();
        let model_index = rendered.find("final answer").unwrap();

        assert!(!rendered.contains("Thinking\n"));
        assert!(!rendered.contains("thinking:"));
        assert!(thinking_index < model_index);
        assert!(!rendered.contains("request-1"));
    }

    #[test]
    fn conversation_hides_low_value_provider_thinking() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer").with_thinking("Answering succinctly."),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();

        assert!(rendered.contains("Answering succinctly"));
        assert!(rendered.contains("final answer"));
    }

    #[test]
    fn conversation_hides_provider_tool_planning_thinking_but_keeps_visible_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("Created the requested files.").with_thinking(
                "Create directory. Use create_directory tool. Path? Desktop relative: Desktop/ElgarLiveE2E.\n\
                 Create file plan.md in that folder. Use create_file.\n\
                 Create files per plan. Use create_file calls for each file. Also need to initialise project?\n\
                 Create files. Provide tool calls for each missing file.\n\
                 Create files with content. Provide tool calls only, one per file.",
            ),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "Created the requested files.",
            AssistantMessageSource::Provider,
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "Desktop/ElgarLiveE2E/plan.md".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("Create directory."));
        assert!(!rendered.contains("Use create_directory tool"));
        assert!(!rendered.contains("Desktop relative"));
        assert!(!rendered.contains("Create file plan.md"));
        assert!(!rendered.contains("Use create_file"));
        assert!(!rendered.contains("Create files per plan"));
        assert!(!rendered.contains("initialise project"));
        assert!(!rendered.contains("Provide tool calls"));
        assert!(rendered.contains("Created the requested files."));
        assert!(rendered.contains("Wrote Desktop/ElgarLiveE2E/plan.md."));
    }

    #[test]
    fn conversation_hides_provider_thinking_for_tool_call_turns() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("").with_thinking(
                "We need to create folder ~/ElgarManualSmoke and set up a TS Next.js Tailwind project.\n\
                 Use write tool for each file. Let's implement.",
            )
            .with_tool_calls(vec![RawModelToolCall {
                id: "call-1".to_string(),
                name: RawModelToolName::Known(ModelToolName::CreateFile),
                arguments: serde_json::json!({
                    "target_path": "package.json",
                    "contents": "{}\n"
                }),
                assistant_summary: None,
            }]),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/ElgarManualSmoke/package.json".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("We need to create folder"));
        assert!(!rendered.contains("Use write tool"));
        assert!(!rendered.contains("Let's implement"));
        assert_eq!(
            rendered,
            "Wrote /Users/yuval/ElgarManualSmoke/package.json."
        );
    }

    #[test]
    fn conversation_summarizes_consecutive_project_create_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-1",
                elgar_core::event::ActionKind::CreateDirectory,
                "create next-tailwind-ts-project",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "/Users/yuval/next-tailwind-ts-project".to_string(),
                },
            ),
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-2",
                elgar_core::event::ActionKind::CreateDirectory,
                "create app",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(
                elgar_core::event::FileActionVerification::DirectoryCreated {
                    path: "/Users/yuval/next-tailwind-ts-project/app".to_string(),
                },
            ),
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-3",
                elgar_core::event::ActionKind::CreateFile,
                "create package.json",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/next-tailwind-ts-project/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApproved(
            ActionEvent::new(
                "action-4",
                elgar_core::event::ActionKind::CreateFile,
                "create app/page.tsx",
            )
            .with_approval_source(elgar_core::policy::ApprovalSource::policy(
                elgar_core::policy::PermissionPolicyMode::AutoCreateReviewModify,
                "safe create",
            )),
        ));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-4",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/next-tailwind-ts-project/app/page.tsx".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\nCreated project: /Users/yuval/next-tailwind-ts-project\nVerified: 2 folders, 2 files"
        );
        assert!(rendered.contains("Tool result"));
        assert!(rendered.contains("Verified: 2 folders, 2 files"));
        assert_eq!(rendered.lines().count(), 3);
        assert_eq!(
            conversation
                .render_lines_with_styles()
                .into_iter()
                .map(|(_line, style)| style)
                .collect::<Vec<_>>(),
            vec![
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool
            ]
        );
        assert!(!rendered.contains("Wrote /Users/yuval/next-tailwind-ts-project/package.json."));
        assert!(!rendered.contains("Wrote /Users/yuval/next-tailwind-ts-project/app/page.tsx."));
        assert!(!rendered.contains("Created /Users/yuval/next-tailwind-ts-project/app."));
        assert_eq!(conversation.render_copy_body(), rendered);
    }

    #[test]
    fn conversation_summarizes_project_create_results_across_interleaved_tool_error() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
                path: "/Users/yuval/__git/elgar/my-nextjs-app".to_string(),
            }),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/next-env.d.ts".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-4",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextapp/next.config.js".to_string(),
            },
        )));
        conversation.push_event(&Event::Error(ErrorEvent::new(
            "model tool `patch_file` is missing required argument `target_path`",
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-5",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\n\
             Created project: /Users/yuval/__git/elgar/my-nextjs-app\n\
             Verified: 1 folder, 4 files\n\
             Outside project: 1 file\n\
             Tool call incomplete: patch_file needs target_path. No action was applied."
        );
        assert_eq!(rendered.matches("Tool result").count(), 1);
        assert_eq!(rendered.matches("Tool call incomplete:").count(), 1);
        assert!(!rendered.contains("Created /Users/yuval/__git/elgar/my-nextjs-app."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/package.json."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/next-env.d.ts."));
        assert!(!rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextapp/next.config.js."));
        assert!(
            !rendered.contains("Wrote /Users/yuval/__git/elgar/my-nextjs-app/tailwind.config.js.")
        );
        assert!(!rendered.contains("model tool `patch_file`"));
        assert_eq!(
            conversation
                .render_lines_with_styles()
                .into_iter()
                .map(|(_line, style)| style)
                .collect::<Vec<_>>(),
            vec![
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Tool,
                ConversationLineStyle::Plain
            ]
        );
    }

    #[test]
    fn conversation_summarizes_project_root_when_first_directory_is_child_folder() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateDirectory,
            VerifiedActionResult::File(FileActionVerification::DirectoryCreated {
                path: "/Users/yuval/__git/elgar/demo/src".to_string(),
            }),
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-2",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/demo/package.json".to_string(),
            },
        )));
        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-3",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "/Users/yuval/__git/elgar/demo/src/App.tsx".to_string(),
            },
        )));

        let rendered = conversation.render_body();

        assert_eq!(
            rendered,
            "Tool result\n\
             Created project: /Users/yuval/__git/elgar/demo\n\
             Verified: 1 folder, 2 files"
        );
        assert!(!rendered.contains("Outside project"));
    }

    #[test]
    fn conversation_keeps_single_create_result_specific() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ActionApplied(ActionApplied::new(
            "action-1",
            elgar_core::event::ActionKind::CreateFile,
            VerifiedActionResult::FileWritten {
                path: "Desktop/ElgarLiveE2E/plan.md".to_string(),
            },
        )));

        assert_eq!(
            conversation.render_body(),
            "Wrote Desktop/ElgarLiveE2E/plan.md."
        );
    }

    #[test]
    fn provider_thinking_filter_catches_tool_planning_without_upstream_help() {
        for hidden in [
            "Use create_directory tool.",
            "Use create_file calls for each file.",
            "Use shellcommand.",
            "Use shell command.",
            "Use write_file tool.",
            "Use planner tool call.",
            "Next tool call: create_file.",
        ] {
            assert!(
                is_low_value_provider_tool_planning_thinking(hidden),
                "{hidden:?} should be hidden"
            );
        }

        for visible in [
            "Reviewing the existing panes tests.",
            "Use clear wording in the final answer.",
            "Checking that normal provider answers remain visible.",
        ] {
            assert!(
                !is_low_value_provider_tool_planning_thinking(visible),
                "{visible:?} should remain visible"
            );
        }
    }

    #[test]
    fn conversation_copy_omits_provider_thinking_blocks_but_keeps_visible_results() {
        let mut conversation = ConversationPane::default();

        conversation.push_line(
            "> Create a folder on my Desktop called ElgarLiveE2E".to_string(),
            ConversationLineStyle::User,
        );
        conversation.push_line(
            "Create directory on Desktop.\n\
             Create file plan.md in that directory.\n\
             Create files per plan: package.json, tsconfig.json, vite.config.ts maybe...\n\
             Create files. We don't have content. Should we ask guidance? Probably need to create files with...\n\
             Call create_file for each target_path with contents. Provide minimal starter files."
                .to_string(),
            ConversationLineStyle::Thinking,
        );
        conversation.push_line("Done.".to_string(), ConversationLineStyle::Plain);
        conversation.push_line(
            "Created Desktop/ElgarLiveE2E.".to_string(),
            ConversationLineStyle::Plain,
        );

        let rendered = conversation.render_body();
        let copied = conversation.render_copy_body();

        assert!(rendered.contains("Create directory on Desktop."));
        assert!(copied.contains("> Create a folder on my Desktop called ElgarLiveE2E"));
        assert!(copied.contains("Done."));
        assert!(copied.contains("Created Desktop/ElgarLiveE2E."));
        assert!(!copied.contains("Create directory on Desktop."));
        assert!(!copied.contains("Create file plan.md in that directory."));
        assert!(!copied.contains("Create files per plan"));
        assert!(!copied.contains("We don't have content"));
        assert!(!copied.contains("Should we ask guidance"));
        assert!(!copied.contains("Call create_file for each target_path"));
        assert!(!copied.contains("Provide minimal starter files"));
    }

    #[test]
    fn conversation_keeps_existing_progress_when_provider_thinking_is_absent() {
        let mut conversation = ConversationPane::default();

        conversation.push_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        conversation.push_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("final answer"),
        )));
        conversation.push_event(&Event::AssistantMessage(AssistantMessage::new(
            "final answer",
            AssistantMessageSource::Provider,
        )));

        let rendered = conversation.render_body();

        assert!(!rendered.contains("thinking"));
        assert!(rendered.contains("final answer"));
        assert!(!rendered.contains("Model:"));
        assert!(!rendered.contains("Thinking\nfinal answer"));
    }

    #[test]
    fn conversation_scrollback_computes_view_offset_without_changing_lines() {
        let mut conversation = ConversationPane {
            lines: (0..10).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };
        let original_lines = conversation.lines.clone();

        assert_eq!(conversation.scroll_offset(4), 6);

        conversation.scroll_up(2);
        assert_eq!(conversation.scroll_offset(4), 4);
        assert_eq!(conversation.lines, original_lines);

        conversation.scroll_down(1);
        assert_eq!(conversation.scroll_offset(4), 5);

        conversation.follow_latest();
        assert_eq!(conversation.scroll_offset(4), 6);
    }

    #[test]
    fn conversation_scrollback_clamps_to_available_content() {
        let mut conversation = ConversationPane {
            lines: (0..3).map(|index| format!("line {index}")).collect(),
            ..ConversationPane::default()
        };

        assert_eq!(conversation.scroll_offset(6), 0);

        conversation.scroll_up(100);
        assert_eq!(conversation.scroll_offset(2), 0);
    }

    #[test]
    fn status_line_distinguishes_provider_and_controller_errors() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::Error(ErrorEvent::new(
            "fake-provider provider request fake-request-1 failed: Provider provider error (404): model missing",
        )));
        assert_eq!(status.render_body(), "provider error");

        status.observe_event(&Event::Error(ErrorEvent::new("Input was not recognized.")));
        assert_eq!(status.render_body(), "error");
    }

    #[test]
    fn status_line_uses_compact_human_readable_provider_text() {
        let mut status = StatusLine::ready();

        status.observe_event(&Event::ProviderStarted(ProviderStarted::new(
            "stub-provider",
            "request-1",
        )));
        assert_eq!(status.text, "◐ working");
        assert!(status.provider_active());

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        assert_eq!(status.text, "reply ready");
        assert!(!status.provider_active());
    }

    #[test]
    fn status_line_cycles_terminal_safe_thinking_pulse() {
        let mut status = StatusLine::ready();

        status.start_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◓ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◑ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◒ working");

        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "◐ working");

        status.observe_event(&Event::ProviderFinished(ProviderFinished::new(
            "stub-provider",
            "request-1",
            ProviderOutput::new("provider text"),
        )));
        status.advance_thinking_pulse();
        assert_eq!(status.render_body(), "reply ready");
    }
}
