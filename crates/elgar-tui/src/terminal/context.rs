use std::path::{Path, PathBuf};

use elgar_core::{
    agent_runtime::AgentRuntime,
    context::ContextAccounting,
    event::ProviderMetrics,
    policy::PermissionPolicyMode,
    provider::ControllerProvider,
    session::Session,
    token_accounting::{ContextWindowSnapshot, ContextWindowSource},
};

use crate::theme;

use super::{
    footer::{align_footer_line, footer_location_label},
    ANSI_CONTEXT_DANGER, ANSI_CONTEXT_MILD, ANSI_CONTEXT_WARNING, ANSI_MUTED,
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
    pub policy_mode: PermissionPolicyMode,
}

impl TerminalShellContext {
    pub fn new(project_root: impl AsRef<Path>, cwd: impl AsRef<Path>) -> Self {
        Self {
            project_root: project_root.as_ref().to_path_buf(),
            cwd: cwd.as_ref().to_path_buf(),
            provider: None,
            model: None,
            provider_metrics: None,
            context_accounting: ContextAccounting::unknown(),
            context_window_snapshot: None,
            policy_mode: PermissionPolicyMode::AutoCreateReviewModify,
        }
    }

    pub fn with_provider(mut self, provider: impl Into<String>, model: Option<String>) -> Self {
        self.provider = Some(provider.into());
        self.model = model;
        self
    }

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

    pub fn with_context_accounting(mut self, context_accounting: ContextAccounting) -> Self {
        self.context_window_snapshot = Some(if context_accounting.estimated_tokens.is_some() {
            ContextWindowSnapshot::from_context_estimate(&context_accounting)
        } else {
            ContextWindowSnapshot::unknown(context_accounting.max_window_tokens)
        });
        self.context_accounting = context_accounting;
        self
    }

    pub fn with_policy_mode(mut self, policy_mode: PermissionPolicyMode) -> Self {
        self.policy_mode = policy_mode;
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

    pub(super) fn footer_body_for_width(&self, width: usize) -> String {
        let left = footer_location_label(&self.project_root, &self.cwd);
        let right = self
            .model
            .as_deref()
            .or(self.provider.as_deref())
            .unwrap_or("");
        if let Some(context_label) = self.footer_context_label() {
            let second_line = if right.is_empty() {
                context_label
            } else {
                format!("{context_label} · {right}")
            };
            return format!("{left}\n{second_line}");
        }

        if right.is_empty() {
            left
        } else {
            align_footer_line(&left, right, width)
        }
    }

    fn footer_context_label(&self) -> Option<String> {
        let snapshot = self.context_window_snapshot.as_ref()?;
        let window = snapshot
            .context_window_tokens
            .map(compact_token_count)
            .unwrap_or_else(|| "?".to_string());
        let percent = footer_percent_label(snapshot);
        if snapshot.source != ContextWindowSource::Provider {
            return Some(format!("{percent}/{window}"));
        }
        let (input, output) = self
            .provider_metrics
            .as_ref()
            .and_then(|metrics| metrics.usage.as_ref())
            .map(|usage| {
                (
                    usage
                        .prompt_tokens
                        .map(compact_token_count)
                        .unwrap_or_else(|| "?".to_string()),
                    usage
                        .completion_tokens
                        .map(compact_token_count)
                        .unwrap_or_else(|| "?".to_string()),
                )
            })
            .unwrap_or_else(|| ("?".to_string(), "?".to_string()));
        Some(format!("↑{input} ↓{output} {percent}/{window}"))
    }

    pub(super) fn footer_ansi(&self) -> &'static str {
        match context_window_pressure(self.context_window_snapshot.as_ref()) {
            ContextWindowPressure::Normal | ContextWindowPressure::Unknown => ANSI_MUTED,
            ContextWindowPressure::Mild => ANSI_CONTEXT_MILD,
            ContextWindowPressure::Warning => ANSI_CONTEXT_WARNING,
            ContextWindowPressure::Danger => ANSI_CONTEXT_DANGER,
        }
    }

    pub(super) fn footer_style(&self) -> ratatui::style::Style {
        match context_window_pressure(self.context_window_snapshot.as_ref()) {
            ContextWindowPressure::Normal | ContextWindowPressure::Unknown => {
                theme::context_normal()
            }
            ContextWindowPressure::Mild => theme::context_mild(),
            ContextWindowPressure::Warning => theme::context_warning(),
            ContextWindowPressure::Danger => theme::context_danger(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextWindowPressure {
    Normal,
    Mild,
    Warning,
    Danger,
    Unknown,
}

pub(crate) fn context_window_pressure(
    snapshot: Option<&ContextWindowSnapshot>,
) -> ContextWindowPressure {
    let Some(snapshot) = snapshot else {
        return ContextWindowPressure::Unknown;
    };
    let Some(percent) = snapshot.used_percent else {
        return ContextWindowPressure::Unknown;
    };

    match percent {
        0..=49 => ContextWindowPressure::Normal,
        50..=69 => ContextWindowPressure::Mild,
        70..=85 => ContextWindowPressure::Warning,
        _ => ContextWindowPressure::Danger,
    }
}

fn compact_token_count(tokens: u64) -> String {
    if tokens >= 1_000 {
        let value = tokens as f64 / 1_000.0;
        if tokens % 1_000 == 0 {
            format!("{value:.0}k")
        } else {
            format!("{value:.1}k")
        }
    } else {
        tokens.to_string()
    }
}

fn footer_percent_label(snapshot: &ContextWindowSnapshot) -> String {
    let percent = match (snapshot.current_tokens, snapshot.context_window_tokens) {
        (Some(current), Some(window)) if window > 0 => {
            let percent = current as f64 * 100.0 / window as f64;
            if percent < 10.0 {
                format!("{percent:.1}%")
            } else {
                format!("{percent:.0}%")
            }
        }
        _ => "?%".to_string(),
    };
    match snapshot.source {
        ContextWindowSource::Estimate => format!("~{percent}"),
        _ => percent,
    }
}

pub(super) fn terminal_context<P>(
    session: &Session,
    runtime: &AgentRuntime<P>,
    policy_mode: PermissionPolicyMode,
) -> TerminalShellContext
where
    P: ControllerProvider,
{
    let mut context = TerminalShellContext::from_session(session);
    context.policy_mode = policy_mode;
    if context.provider.is_none() {
        let request = runtime.provider.request_metadata();
        context.provider = Some(request.provider);
        context.model = request.model;
    }
    context
}

pub(super) fn default_no_network_line() -> &'static str {
    "default no-network stub"
}
