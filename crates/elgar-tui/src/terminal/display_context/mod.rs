//! Runtime display context shown in the terminal footer and startup display.
//!
//! This is TUI display state about paths, provider profile, session metrics,
//! and context-window accounting. It is not the model prompt context itself.

use std::path::{Path, PathBuf};

use elgar_core::{
    context::ContextAccounting,
    event::ProviderMetrics,
    provider::ControllerProvider,
    session::Session,
    token_accounting::{ContextWindowSnapshot, ContextWindowSource},
};

use crate::theme;

use super::{
    ui::{
        approval::{
            render_approval_footer_actions_for_tool, render_pending_approval_footer_actions,
        },
        approval_action::ApprovalAction,
        footer::{align_footer_line, footer_location_label},
    },
    ANSI_MUTED,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalShellContext {
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub provider_metrics: Option<ProviderMetrics>,
    pub context_accounting: ContextAccounting,
    pub context_window_snapshot: Option<ContextWindowSnapshot>,
    pub approval_tool: Option<String>,
    pub approval_actions_line: Option<String>,
}

impl TerminalShellContext {
    /// Create context with only project/cwd path information.
    pub fn new(project_root: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            cwd: cwd.as_ref().to_path_buf(),
            provider: None,
            model: None,
            provider_metrics: None,
            context_accounting: ContextAccounting::unknown(),
            context_window_snapshot: None,
            approval_tool: None,
            approval_actions_line: None,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>, model: Option<String>) -> Self {
        self.provider = Some(provider.into());
        self.model = model;
        self
    }

    /// Rebuild the visible terminal context from current session metadata.
    pub fn from_session(session: &Session) -> Self {
        let mut context = Self::new(&session.project_root, &session.cwd);
        if let Some(metadata) = session.provider_metadata() {
            context.provider = Some(metadata.provider.clone());
            context.model = metadata.model.clone();
            context.provider_metrics = metadata.metrics.clone();
        }
        context.context_accounting = session.context_accounting().clone();
        context.context_window_snapshot = Some(session.latest_context_window_snapshot());
        context
    }

    /// Attach local context accounting and derive a context-window snapshot.
    pub fn with_context_accounting(mut self, context_accounting: ContextAccounting) -> Self {
        self.context_window_snapshot = Some(if context_accounting.estimated_tokens.is_some() {
            ContextWindowSnapshot::from_context_estimate(&context_accounting)
        } else {
            ContextWindowSnapshot::unknown(context_accounting.max_window_tokens)
        });
        self.context_accounting = context_accounting;
        self
    }

    #[cfg(test)]
    pub fn with_provider_metrics(mut self, provider_metrics: ProviderMetrics) -> Self {
        self.provider_metrics = Some(provider_metrics);
        self
    }

    pub(crate) fn footer_body(&self, _status: &str, _copy_hint: &str) -> String {
        self.footer_body_for_width(80)
    }

    pub(crate) fn with_approval_action_selected(mut self, selected: ApprovalAction) -> Self {
        self.approval_actions_line = self
            .approval_tool
            .as_ref()
            .map(|tool| render_approval_footer_actions_for_tool(tool, selected));
        self
    }

    pub(super) fn footer_body_for_width(&self, width: usize) -> String {
        let left = footer_location_label(&self.project_root, &self.cwd);
        let model = self
            .model
            .as_deref()
            .or(self.provider.as_deref())
            .unwrap_or("");
        let window = footer_context_window_label(self.context_window_snapshot.as_ref());
        let right = match (window.as_deref(), model.is_empty()) {
            (Some(window), false) => format!("{window} · {model}"),
            (Some(window), true) => window.to_string(),
            (None, false) => model.to_string(),
            (None, true) => String::new(),
        };
        let base = if right.is_empty() {
            left
        } else {
            align_footer_line(&left, &right, width)
        };
        match self.approval_actions_line.as_deref() {
            Some(actions) => format!("{base}\n{actions}"),
            None => base,
        }
    }

    pub(super) fn footer_ansi(&self) -> &'static str {
        ANSI_MUTED
    }

    pub(super) fn footer_style(&self) -> ratatui::style::Style {
        theme::context_normal()
    }
}

fn footer_context_window_label(snapshot: Option<&ContextWindowSnapshot>) -> Option<String> {
    let snapshot = snapshot?;
    let window = snapshot.context_window_tokens?;
    let current = match snapshot.source {
        ContextWindowSource::Provider => snapshot
            .current_tokens
            .map(format_compact_tokens)
            .unwrap_or_else(|| "?".to_string()),
        ContextWindowSource::Estimate | ContextWindowSource::Unknown => "?".to_string(),
    };
    Some(format!("{current}/{}", format_compact_tokens(window)))
}

fn format_compact_tokens(tokens: u64) -> String {
    if tokens >= 1_000 {
        let value = tokens as f64 / 1_000.0;
        if tokens.is_multiple_of(1_000) {
            format!("{}k", tokens / 1_000)
        } else {
            format!("{value:.1}k")
        }
    } else {
        tokens.to_string()
    }
}

pub(super) fn terminal_context<P>(session: &Session, provider: &P) -> TerminalShellContext
where
    P: ControllerProvider,
{
    let mut context = TerminalShellContext::from_session(session);
    if context.provider.is_none() {
        let request = provider.request_metadata();
        context.provider = Some(request.provider);
        context.model = request.model;
    }
    if let Some(approval) = session.pending_approval() {
        context.approval_tool = Some(approval.tool.clone());
        context.approval_actions_line = Some(render_pending_approval_footer_actions(
            approval,
            ApprovalAction::Approve,
        ));
    }
    context
}

pub(super) fn default_no_network_line() -> &'static str {
    "default no-network stub"
}
