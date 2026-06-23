use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    action::{Action, ActionLifecycleState},
    context::ContextAccounting,
    event::{
        ActionEvent, ActionKind, Event, FileActionVerification, ProviderMetrics,
        VerifiedActionResult,
    },
    local_session_log, local_trace,
    plan_contract::PlanContract,
    policy::PolicyDecision,
    token_accounting::{ContextWindowSnapshot, LastTurnTokenUsage, SessionTokenTotals},
};

/// Core-owned state for one controller session.
///
/// This is an inspectable record of controller facts. Provider events and
/// metadata may capture what a provider said or which provider was used, but
/// they do not prove filesystem state, action success, or verified results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project_root: PathBuf,
    pub cwd: PathBuf,
    events: Vec<Event>,
    actions: Vec<ActionRecord>,
    provider_metadata: Option<ProviderMetadata>,
    #[serde(default)]
    project_memory: ProjectMemory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_provider_prompt_memory_selection: Option<ProviderPromptMemorySelection>,
    #[serde(default)]
    plan_contracts: Vec<PlanContract>,
    #[serde(default)]
    context_accounting: ContextAccounting,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_context_window_snapshot: Option<ContextWindowSnapshot>,
    #[serde(default)]
    session_token_totals: SessionTokenTotals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_turn_token_usage: Option<LastTurnTokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_turn_perf_summary: Option<TurnPerfSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_reasoning_trace: Option<ReasoningTrace>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_runtime_block: Option<RuntimeBlockRecord>,
    /// Monotonic counter incremented at the start of each turn.
    ///
    /// Actions are stamped with the turn that produced them so verified-state
    /// answers can report the most recent action-producing turn instead of the
    /// cumulative session inventory.
    #[serde(default)]
    current_turn_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_trace_id: Option<String>,
    #[serde(default)]
    current_turn_event_start_index: usize,
    #[serde(default)]
    current_turn_action_start_index: usize,
}

pub const PROJECT_MEMORY_LIMIT: usize = 8;
pub const PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT: usize = PROJECT_MEMORY_LIMIT;

impl Session {
    pub fn new(
        id: impl Into<String>,
        project_root: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        Self {
            id: id.into(),
            project_root: project_root.into(),
            cwd: cwd.into(),
            events: Vec::new(),
            actions: Vec::new(),
            provider_metadata: None,
            project_memory: ProjectMemory::default(),
            latest_provider_prompt_memory_selection: None,
            plan_contracts: Vec::new(),
            context_accounting: ContextAccounting::unknown(),
            latest_context_window_snapshot: None,
            session_token_totals: SessionTokenTotals::default(),
            latest_turn_token_usage: None,
            latest_turn_perf_summary: None,
            latest_reasoning_trace: None,
            latest_runtime_block: None,
            current_turn_index: 0,
            current_trace_id: None,
            current_turn_event_start_index: 0,
            current_turn_action_start_index: 0,
        }
    }

    /// Controller-recorded event facts for read-only UI and renderer consumers.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Controller-owned action records for read-only UI and renderer consumers.
    pub fn actions(&self) -> &[ActionRecord] {
        &self.actions
    }

    pub fn current_turn_index(&self) -> u64 {
        self.current_turn_index
    }

    /// Action records belonging to the most recent turn that produced a
    /// verified action.
    ///
    /// Used to answer "what did you just do/create" without leaking earlier
    /// turns' cumulative inventory into a single-turn answer. Questions asked in
    /// their own (action-free) turn still resolve to the last turn that acted.
    pub fn actions_in_latest_action_turn(&self) -> Vec<&ActionRecord> {
        let Some(latest_turn) = self
            .actions
            .iter()
            .filter(|record| record.verified_result.is_some())
            .map(|record| record.turn_index)
            .max()
        else {
            return Vec::new();
        };
        self.actions
            .iter()
            .filter(|record| record.turn_index == latest_turn)
            .collect()
    }

    /// Select the one action still waiting on user approval/rejection.
    ///
    /// Only `Proposed` actions are pending. `Approved`, `Applied`, `Rejected`,
    /// and `Failed` records are non-pending for selection, including when a
    /// session is restored with those states already present.
    pub fn pending_action_selection(&self) -> PendingActionSelection {
        let mut proposed = self
            .actions
            .iter()
            .enumerate()
            .filter(|(_index, record)| record.action.state == ActionLifecycleState::Proposed);

        let Some((index, _record)) = proposed.next() else {
            return PendingActionSelection::None;
        };

        if proposed.next().is_some() {
            PendingActionSelection::Ambiguous
        } else {
            PendingActionSelection::Single(index)
        }
    }

    /// Provider request metadata recorded by the controller for inspection only.
    pub fn provider_metadata(&self) -> Option<&ProviderMetadata> {
        self.provider_metadata.as_ref()
    }

    /// Controller-owned project references learned only from verified actions.
    pub fn project_memory(&self) -> &ProjectMemory {
        &self.project_memory
    }

    /// Latest provider prompt memory selection trace recorded by the controller.
    pub fn latest_provider_prompt_memory_selection(
        &self,
    ) -> Option<&ProviderPromptMemorySelection> {
        self.latest_provider_prompt_memory_selection.as_ref()
    }

    /// First-class planning contracts owned by core runtime state.
    pub fn plan_contracts(&self) -> &[PlanContract] {
        &self.plan_contracts
    }

    /// Latest first-class planning contract, if one has been recorded.
    pub fn latest_plan_contract(&self) -> Option<&PlanContract> {
        self.plan_contracts.last()
    }

    /// Controller-recorded context accounting for UI display and provider budgeting.
    pub fn context_accounting(&self) -> &ContextAccounting {
        &self.context_accounting
    }

    pub fn latest_context_window_snapshot(&self) -> ContextWindowSnapshot {
        self.latest_context_window_snapshot
            .clone()
            .unwrap_or_else(|| {
                if self.context_accounting.estimated_tokens.is_some() {
                    ContextWindowSnapshot::from_context_estimate(&self.context_accounting)
                } else {
                    ContextWindowSnapshot::unknown(self.context_accounting.max_window_tokens)
                }
            })
    }

    pub fn session_token_totals(&self) -> &SessionTokenTotals {
        &self.session_token_totals
    }

    pub fn latest_turn_token_usage(&self) -> Option<&LastTurnTokenUsage> {
        self.latest_turn_token_usage.as_ref()
    }

    pub fn latest_turn_perf_summary(&self) -> Option<&TurnPerfSummary> {
        self.latest_turn_perf_summary.as_ref()
    }

    /// Latest inspectable reasoning/decision trace for user review.
    ///
    /// This is debug visibility, not proof of filesystem state. Verified action
    /// records remain the source of truth for what actually happened.
    pub fn latest_reasoning_trace(&self) -> Option<&ReasoningTrace> {
        self.latest_reasoning_trace.as_ref()
    }

    pub fn latest_runtime_block(&self) -> Option<&RuntimeBlockRecord> {
        self.latest_runtime_block.as_ref()
    }

    pub(crate) fn latest_runtime_block_if_recent(&self) -> Option<&RuntimeBlockRecord> {
        self.latest_runtime_block
            .as_ref()
            .filter(|block| block.turn_index.saturating_add(1) >= self.current_turn_index)
    }

    pub(crate) fn push_event(&mut self, event: Event) {
        log_session_event(&self.id, self.current_turn_index, &event);
        self.trace_event_for_controller_event(&event);
        self.events.push(event);
    }

    pub(crate) fn push_action(&mut self, mut action: ActionRecord) {
        action.turn_index = self.current_turn_index;
        self.actions.push(action);
    }

    pub(crate) fn action_mut(&mut self, index: usize) -> Option<&mut ActionRecord> {
        self.actions.get_mut(index)
    }

    pub(crate) fn set_provider_metadata(&mut self, metadata: ProviderMetadata) {
        self.provider_metadata = Some(metadata);
    }

    pub(crate) fn set_context_accounting(&mut self, context_accounting: ContextAccounting) {
        self.context_accounting = context_accounting;
    }

    pub(crate) fn record_provider_metrics(&mut self, metrics: &ProviderMetrics) {
        self.trace_event(
            "token_usage",
            json!({
                "request_id": &metrics.request_id,
                "model": &metrics.model,
                "stream": metrics.stream,
                "message_count": metrics.message_count,
                "serialized_request_bytes": metrics.serialized_request_bytes,
                "backend": &metrics.backend,
                "reasoning": &metrics.reasoning,
                "context_length": metrics.context_length,
                "stats": metrics.stats,
                "provider_time_to_first_token_millis": metrics.provider_time_to_first_token_millis,
                "provider_tokens_per_second_milli": metrics.provider_tokens_per_second_milli,
                "reasoning_output_tokens": metrics.reasoning_output_tokens,
                "prompt_tokens": metrics.usage.as_ref().and_then(|usage| usage.prompt_tokens),
                "completion_tokens": metrics.usage.as_ref().and_then(|usage| usage.completion_tokens),
                "total_tokens": metrics.usage.as_ref().and_then(|usage| usage.total_tokens),
                "first_chunk_latency_millis": metrics.first_chunk_latency_millis,
                "total_duration_millis": metrics.total_duration_millis,
            }),
        );
        let Some(usage) = metrics.usage.as_ref() else {
            return;
        };
        self.latest_context_window_snapshot = Some(ContextWindowSnapshot::from_provider_usage(
            usage,
            self.context_accounting.max_window_tokens,
            metrics.request_id.clone(),
        ));
        self.session_token_totals.add_provider_usage(usage);
        self.latest_turn_token_usage = LastTurnTokenUsage::from_provider_metrics(metrics);
    }

    pub(crate) fn start_reasoning_trace(&mut self, user_input: impl Into<String>) {
        let user_input = user_input.into();
        self.current_turn_index = self.current_turn_index.saturating_add(1);
        self.current_trace_id = Some(local_trace::new_trace_id(&self.id, self.current_turn_index));
        self.current_turn_event_start_index = self.events.len();
        self.current_turn_action_start_index = self.actions.len();
        self.latest_reasoning_trace = Some(ReasoningTrace::new(user_input.clone()));
        self.trace_event(
            "turn_start",
            json!({
                "input_chars": user_input.chars().count(),
                "input_lines": user_input.lines().count().max(1),
            }),
        );
        self.session_log_event(
            "user_message",
            json!({
                "content_chars": user_input.chars().count(),
                "content_lines": user_input.lines().count().max(1),
            }),
        );
    }

    pub(crate) fn record_reasoning_route(&mut self, route: impl Into<String>) {
        let route = route.into();
        if let Some(trace) = self.latest_reasoning_trace.as_mut() {
            trace.route = Some(route.clone());
        }
        self.trace_event("route_decision", json!({ "route": route }));
    }

    pub(crate) fn push_reasoning_provider_planning(&mut self, line: impl Into<String>) {
        if let Some(trace) = self.latest_reasoning_trace.as_mut() {
            trace.push_provider_planning(line);
        }
    }

    pub(crate) fn push_reasoning_model_decision(&mut self, line: impl Into<String>) {
        if let Some(trace) = self.latest_reasoning_trace.as_mut() {
            trace.push_model_decision(line);
        }
    }

    pub(crate) fn push_reasoning_runtime_check(&mut self, line: impl Into<String>) {
        if let Some(trace) = self.latest_reasoning_trace.as_mut() {
            trace.push_runtime_check(line);
        }
    }

    pub(crate) fn record_runtime_block(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.latest_runtime_block = Some(RuntimeBlockRecord {
            turn_index: self.current_turn_index,
            message: message.clone(),
        });
        self.trace_event(
            "runtime_block",
            json!({
                "message_chars": message.chars().count(),
                "category": "runtime_block",
            }),
        );
    }

    pub(crate) fn clear_runtime_block(&mut self) {
        self.latest_runtime_block = None;
    }

    pub(crate) fn record_verified_folder_reference(&mut self, reference: VerifiedFolderReference) {
        self.project_memory.remember_verified_folder(reference);
    }

    pub(crate) fn record_verified_plan_reference(&mut self, reference: VerifiedPlanReference) {
        self.project_memory.remember_verified_plan(reference);
    }

    pub(crate) fn record_structured_project_plan(&mut self, plan: StructuredProjectPlan) {
        let contract_id = plan_contract_id_from_structured_plan(&plan);
        let contract = PlanContract::draft_from_structured_plan(contract_id, &plan);
        self.project_memory.remember_structured_plan(plan);
        self.record_plan_contract(contract);
    }

    pub fn record_plan_contract(&mut self, contract: PlanContract) {
        self.plan_contracts
            .retain(|existing| existing.id != contract.id);
        self.plan_contracts.push(contract);
        trim_to_memory_limit(&mut self.plan_contracts);
    }

    pub(crate) fn set_latest_provider_prompt_memory_selection(
        &mut self,
        selection: Option<ProviderPromptMemorySelection>,
    ) {
        self.latest_provider_prompt_memory_selection = selection.map(|mut selection| {
            selection.bound();
            selection
        });
        if let Some(selection) = self.latest_provider_prompt_memory_selection.as_ref() {
            self.trace_event(
                "memory_selected",
                json!({
                    "selected_count": selection.selected.len(),
                    "omitted_count": selection.omitted.len(),
                    "selected": selection.selected.iter().map(provider_memory_selected_trace_value).collect::<Vec<_>>(),
                    "omitted": selection.omitted.iter().map(provider_memory_omitted_trace_value).collect::<Vec<_>>(),
                }),
            );
        }
    }

    pub(crate) fn trace_event(&self, kind: impl Into<String>, metadata: Value) {
        let Some(trace_id) = self.current_trace_id.as_deref() else {
            return;
        };
        let kind = kind.into();
        let _ = local_trace::append_trace_event(
            &self.project_root,
            &self.id,
            trace_id,
            self.current_turn_index,
            kind.clone(),
            metadata.clone(),
        );
        self.session_log_event(kind, metadata);
    }

    pub(crate) fn session_log_event(&self, kind: impl Into<String>, metadata: Value) {
        let _ = local_session_log::append_session_event(
            &self.project_root,
            &self.id,
            self.current_turn_index,
            kind,
            metadata,
        );
    }

    pub(crate) fn finish_trace_turn(&mut self) {
        if self.current_trace_id.is_none() {
            return;
        }
        let route = self
            .latest_reasoning_trace
            .as_ref()
            .and_then(|trace| trace.route.clone());
        let turn_events = &self.events[self.current_turn_event_start_index..];
        let turn_actions = &self.actions[self.current_turn_action_start_index..];
        let perf_summary = TurnPerfSummary::from_turn(route.clone(), turn_events, turn_actions);
        self.trace_event(
            "turn_perf_summary",
            serde_json::to_value(&perf_summary).unwrap_or_else(|_| json!({})),
        );
        self.latest_turn_perf_summary = Some(perf_summary);
        self.trace_event(
            "turn_finish",
            json!({
                "route": route,
                "event_count": self.events.len(),
                "action_count": self.actions.len(),
            }),
        );
        self.current_trace_id = None;
    }

    fn trace_event_for_controller_event(&self, event: &Event) {
        match event {
            Event::ProviderStarted(started) => self.trace_event(
                "provider_request_start",
                json!({
                    "provider": &started.provider,
                    "request_id": &started.request_id,
                    "model": &started.model,
                    "request_mode": &started.request_mode,
                    "tool_count": started.tool_count,
                    "backend": &started.backend,
                    "reasoning": &started.reasoning,
                    "context_length": started.context_length,
                    "stats": started.stats,
                }),
            ),
            Event::ProviderFinished(finished) => self.trace_event(
                "provider_request_finish",
                json!({
                    "provider": &finished.provider,
                    "request_id": &finished.request_id,
                    "text_chars": finished.output.text.chars().count(),
                    "thinking_chars": finished.output.thinking.as_ref().map(|value| value.chars().count()),
                    "tool_call_count": finished.output.tool_calls.len(),
                    "tool_names": finished.output.tool_calls.iter().map(|tool_call| tool_call.name.raw_label()).collect::<Vec<_>>(),
                    "has_metrics": finished.output.metrics.is_some(),
                }),
            ),
            Event::ActionProposed(action) => self.trace_action_event("action_proposed", action),
            Event::ActionApproved(action) => self.trace_action_event("action_approved", action),
            Event::ActionRejected(action) => self.trace_action_event("action_rejected", action),
            Event::ActionApplied(applied) => self.trace_event(
                "action_applied",
                action_applied_trace_metadata(applied.action_id.as_str(), applied.action_kind, &applied.result),
            ),
            Event::ActionFailed(failed) => self.trace_event(
                "action_failed",
                json!({
                    "action_id": &failed.action_id,
                    "action_kind": format!("{:?}", failed.action_kind),
                    "reason_chars": failed.reason.chars().count(),
                    "category": "action_failed",
                }),
            ),
            Event::Error(error) => self.trace_event(
                "provider_error",
                json!({
                    "message_chars": error.message.chars().count(),
                    "category": "provider_or_runtime_error",
                }),
            ),
            Event::UserMessage(_) => {}
            Event::AssistantMessage(message) => self.session_log_event(
                "assistant_message",
                json!({
                    "source": format!("{:?}", message.source),
                    "content_chars": message.content.chars().count(),
                    "content_lines": message.content.lines().count().max(1),
                }),
            ),
        }
    }

    fn trace_action_event(&self, kind: &str, action: &ActionEvent) {
        let mut metadata = json!({
            "action_id": &action.action_id,
            "action_kind": format!("{:?}", action.action_kind),
            "summary_chars": action.summary.chars().count(),
        });
        if let Some(object) = metadata.as_object_mut() {
            match (action.action_kind, action.target.as_deref()) {
                (ActionKind::ShellCommand, Some(target)) => {
                    object.insert("command".to_string(), json!(target));
                    object.insert("command_chars".to_string(), json!(target.chars().count()));
                    if let Some(details) = action.shell_details.as_ref() {
                        object.insert("cwd".to_string(), json!(&details.cwd));
                        object.insert(
                            "timeout_seconds".to_string(),
                            json!(details.timeout_seconds),
                        );
                        object.insert(
                            "expected_effect_chars".to_string(),
                            json!(details.expected_effect.chars().count()),
                        );
                    }
                }
                (_, Some(target)) => {
                    object.insert("target".to_string(), json!(target));
                }
                _ => {}
            }
        }
        self.trace_event(kind, metadata);
    }

    pub(crate) fn mark_structured_project_plan_executed(&mut self, action_id: &str) {
        self.project_memory.mark_structured_plan_executed(action_id);
    }

    pub(crate) fn mark_latest_structured_project_plan_executing(&mut self) {
        self.project_memory
            .mark_latest_structured_plan_status(StructuredProjectPlanStatus::Executing);
    }

    pub(crate) fn mark_latest_structured_project_plan_completed(&mut self) {
        self.project_memory
            .mark_latest_structured_plan_status(StructuredProjectPlanStatus::Completed);
    }

    pub(crate) fn remove_structured_project_plan_for_action(&mut self, action_id: &str) {
        self.project_memory
            .remove_structured_plan_for_action(action_id);
    }
}

fn plan_contract_id_from_structured_plan(plan: &StructuredProjectPlan) -> String {
    plan.source_action_id
        .as_ref()
        .map(|source_action_id| format!("plan-contract:{source_action_id}"))
        .unwrap_or_else(|| format!("plan-contract:{}", plan.source_plan_path.to_string_lossy()))
}

fn provider_memory_selected_trace_value(fact: &ProviderPromptMemorySelectedFact) -> Value {
    json!({
        "kind": &fact.kind,
        "path": &fact.path,
        "project_root": &fact.project_root,
        "source_action_id": &fact.source_action_id,
    })
}

fn provider_memory_omitted_trace_value(fact: &ProviderPromptMemoryOmittedFact) -> Value {
    json!({
        "kind": &fact.kind,
        "path": &fact.path,
        "project_root": &fact.project_root,
        "source_action_id": &fact.source_action_id,
        "reason": &fact.reason,
    })
}

fn log_session_event(session_id: &str, turn_index: u64, event: &Event) {
    match event {
        Event::UserMessage(message) => log::debug!(
            "session_event session={} turn={} kind=user_message content_chars={}",
            session_id,
            turn_index,
            message.content.chars().count()
        ),
        Event::AssistantMessage(message) => log::debug!(
            "session_event session={} turn={} kind=assistant_message source={:?} content_chars={}",
            session_id,
            turn_index,
            message.source,
            message.content.chars().count()
        ),
        Event::ProviderStarted(started) => log::info!(
            "session_event session={} turn={} kind=provider_started provider={} request_id={} mode={} tools={} backend={}",
            session_id,
            turn_index,
            started.provider,
            started.request_id,
            started.request_mode.as_deref().unwrap_or("n/a"),
            started.tool_count.unwrap_or(0),
            started
                .backend
                .as_ref()
                .map(|backend| format!("{backend:?}"))
                .unwrap_or_else(|| "n/a".to_string())
        ),
        Event::ProviderFinished(finished) => log::info!(
            "session_event session={} turn={} kind=provider_finished provider={} request_id={} text_chars={} thinking_chars={} tool_calls={}",
            session_id,
            turn_index,
            finished.provider,
            finished.request_id,
            finished.output.text.chars().count(),
            finished
                .output
                .thinking
                .as_ref()
                .map(|thinking| thinking.chars().count())
                .unwrap_or(0),
            finished.output.tool_calls.len()
        ),
        Event::ActionProposed(action) => log::info!(
            "session_event session={} turn={} kind=action_proposed action_id={} action_kind={:?} target={}",
            session_id,
            turn_index,
            action.action_id,
            action.action_kind,
            action.target.as_deref().unwrap_or("n/a")
        ),
        Event::ActionApproved(action) => log::info!(
            "session_event session={} turn={} kind=action_approved action_id={} action_kind={:?} source={}",
            session_id,
            turn_index,
            action.action_id,
            action.action_kind,
            action
                .approval_source
                .as_ref()
                .map(|source| format!("{source:?}"))
                .unwrap_or_else(|| "n/a".to_string())
        ),
        Event::ActionRejected(action) => log::info!(
            "session_event session={} turn={} kind=action_rejected action_id={} action_kind={:?}",
            session_id,
            turn_index,
            action.action_id,
            action.action_kind
        ),
        Event::ActionApplied(applied) => match &applied.result {
            VerifiedActionResult::Shell(shell) => log::info!(
                "session_event session={} turn={} kind=action_applied action_id={} action_kind={:?} operation=shell exit_code={:?} elapsed_ms={} stdout_bytes={} stderr_bytes={}",
                session_id,
                turn_index,
                applied.action_id,
                applied.action_kind,
                shell.exit_code,
                shell.elapsed_millis,
                shell.stdout.len(),
                shell.stderr.len()
            ),
            VerifiedActionResult::FileWritten { path } => log::info!(
                "session_event session={} turn={} kind=action_applied action_id={} action_kind={:?} operation=file_written path={}",
                session_id,
                turn_index,
                applied.action_id,
                applied.action_kind,
                path
            ),
            VerifiedActionResult::File(verification) => log::info!(
                "session_event session={} turn={} kind=action_applied action_id={} action_kind={:?} operation=file verification={:?}",
                session_id,
                turn_index,
                applied.action_id,
                applied.action_kind,
                verification
            ),
        },
        Event::ActionFailed(failed) => log::error!(
            "session_event session={} turn={} kind=action_failed action_id={} action_kind={:?} reason_chars={}",
            session_id,
            turn_index,
            failed.action_id,
            failed.action_kind,
            failed.reason.chars().count()
        ),
        Event::Error(error) => log::error!(
            "session_event session={} turn={} kind=error message={}",
            session_id,
            turn_index,
            error.message
        ),
    }
}

fn action_applied_trace_metadata(
    action_id: &str,
    action_kind: ActionKind,
    result: &VerifiedActionResult,
) -> Value {
    let mut metadata = json!({
        "action_id": action_id,
        "action_kind": format!("{:?}", action_kind),
    });
    if let Some(object) = metadata.as_object_mut() {
        match result {
            VerifiedActionResult::FileWritten { path } => {
                object.insert("operation".to_string(), json!("file_written"));
                object.insert("path".to_string(), json!(path));
            }
            VerifiedActionResult::File(verification) => match verification {
                FileActionVerification::FileCreated { path } => {
                    object.insert("operation".to_string(), json!("file_created"));
                    object.insert("path".to_string(), json!(path));
                }
                FileActionVerification::FilePatched { path } => {
                    object.insert("operation".to_string(), json!("file_patched"));
                    object.insert("path".to_string(), json!(path));
                }
                FileActionVerification::FileOverwritten { path } => {
                    object.insert("operation".to_string(), json!("file_overwritten"));
                    object.insert("path".to_string(), json!(path));
                }
                FileActionVerification::FileDeleted { path } => {
                    object.insert("operation".to_string(), json!("file_deleted"));
                    object.insert("path".to_string(), json!(path));
                }
                FileActionVerification::FileMoved {
                    source_path,
                    target_path,
                } => {
                    object.insert("operation".to_string(), json!("file_moved"));
                    object.insert("source_path".to_string(), json!(source_path));
                    object.insert("path".to_string(), json!(target_path));
                }
                FileActionVerification::DirectoryCreated { path } => {
                    object.insert("operation".to_string(), json!("directory_created"));
                    object.insert("path".to_string(), json!(path));
                }
            },
            VerifiedActionResult::Shell(verification) => {
                object.insert("operation".to_string(), json!("shell_command"));
                object.insert("command".to_string(), json!(&verification.command));
                object.insert("cwd".to_string(), json!(&verification.cwd));
                object.insert("exit_code".to_string(), json!(verification.exit_code));
                object.insert(
                    "elapsed_millis".to_string(),
                    json!(verification.elapsed_millis),
                );
                object.insert("timed_out".to_string(), json!(verification.timed_out));
                object.insert("stdout_bytes".to_string(), json!(verification.stdout.len()));
                object.insert("stderr_bytes".to_string(), json!(verification.stderr.len()));
                object.insert(
                    "verified_effect_present".to_string(),
                    json!(verification.verified_effect.is_some()),
                );
                object.insert(
                    "command_chars".to_string(),
                    json!(verification.command.chars().count()),
                );
                object.insert(
                    "stdout_tail".to_string(),
                    json!(trace_output_tail(&verification.stdout)),
                );
                object.insert(
                    "stderr_tail".to_string(),
                    json!(trace_output_tail(&verification.stderr)),
                );
            }
        }
    }
    metadata
}

fn trace_output_tail(output: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let chars = output.chars().collect::<Vec<_>>();
    let start = chars.len().saturating_sub(MAX_CHARS);
    chars[start..].iter().collect()
}

/// Compact per-turn performance facts derived from verified runtime events.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnPerfSummary {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub provider_request_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub request_modes: Vec<String>,
    pub total_tool_count: usize,
    pub action_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_provider_duration_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_chunk_latency_millis: Option<u64>,
    pub message_count: usize,
    pub serialized_request_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    pub visible_text_chars: usize,
    pub thinking_chars: usize,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_requests: Vec<ProviderRequestPerfSummary>,
}

impl TurnPerfSummary {
    fn from_turn(
        route: Option<String>,
        events: &[Event],
        actions: &[ActionRecord],
    ) -> TurnPerfSummary {
        let mut summary = TurnPerfSummary {
            route,
            action_count: actions.len(),
            ..TurnPerfSummary::default()
        };

        for event in events {
            match event {
                Event::ProviderStarted(started) => {
                    summary.provider_requests.push(ProviderRequestPerfSummary {
                        request_id: started.request_id.clone(),
                        provider: started.provider.clone(),
                        model: started.model.clone(),
                        request_mode: started.request_mode.clone(),
                        tool_count: started.tool_count.unwrap_or(0),
                        backend: started.backend,
                        reasoning: started.reasoning,
                        context_length: started.context_length,
                        stats: started.stats,
                        ..ProviderRequestPerfSummary::default()
                    });
                }
                Event::ProviderFinished(finished) => {
                    let request_index = summary
                        .provider_requests
                        .iter()
                        .position(|request| request.request_id == finished.request_id)
                        .unwrap_or_else(|| {
                            summary.provider_requests.push(ProviderRequestPerfSummary {
                                request_id: finished.request_id.clone(),
                                provider: finished.provider.clone(),
                                ..ProviderRequestPerfSummary::default()
                            });
                            summary.provider_requests.len() - 1
                        });
                    let request = &mut summary.provider_requests[request_index];
                    request.visible_text_chars = finished.output.text.chars().count();
                    request.thinking_chars = finished
                        .output
                        .thinking
                        .as_ref()
                        .map(|value| value.chars().count())
                        .unwrap_or(0);
                    request.tool_call_count = finished.output.tool_calls.len();
                    request.tool_names = finished
                        .output
                        .tool_calls
                        .iter()
                        .map(|tool_call| tool_call.name.raw_label())
                        .collect();
                    if let Some(metrics) = finished.output.metrics.as_ref() {
                        request.model = request.model.clone().or_else(|| metrics.model.clone());
                        request.stream = metrics.stream;
                        request.message_count = metrics.message_count;
                        request.serialized_request_bytes = metrics.serialized_request_bytes;
                        request.backend = request.backend.or(metrics.backend);
                        request.reasoning = request.reasoning.or(metrics.reasoning);
                        request.context_length = request.context_length.or(metrics.context_length);
                        request.stats = request.stats.or(metrics.stats);
                        request.provider_time_to_first_token_millis =
                            metrics.provider_time_to_first_token_millis;
                        request.provider_tokens_per_second_milli =
                            metrics.provider_tokens_per_second_milli;
                        request.reasoning_output_tokens = metrics.reasoning_output_tokens;
                        request.total_duration_millis = metrics.total_duration_millis;
                        request.first_chunk_latency_millis = metrics.first_chunk_latency_millis;
                        request.prompt_tokens =
                            metrics.usage.as_ref().and_then(|usage| usage.prompt_tokens);
                        request.completion_tokens = metrics
                            .usage
                            .as_ref()
                            .and_then(|usage| usage.completion_tokens);
                        request.total_tokens =
                            metrics.usage.as_ref().and_then(|usage| usage.total_tokens);
                    }
                }
                _ => {}
            }
        }

        summary.provider_request_count = summary.provider_requests.len();
        summary.request_modes = summary
            .provider_requests
            .iter()
            .filter_map(|request| request.request_mode.clone())
            .collect();
        summary.total_tool_count = summary
            .provider_requests
            .iter()
            .map(|request| request.tool_count)
            .sum();
        summary.total_provider_duration_millis = sum_options(
            summary
                .provider_requests
                .iter()
                .map(|request| request.total_duration_millis),
        );
        summary.first_chunk_latency_millis = min_options(
            summary
                .provider_requests
                .iter()
                .map(|request| request.first_chunk_latency_millis),
        );
        summary.message_count = summary
            .provider_requests
            .iter()
            .map(|request| request.message_count)
            .sum();
        summary.serialized_request_bytes = summary
            .provider_requests
            .iter()
            .map(|request| request.serialized_request_bytes)
            .sum();
        summary.prompt_tokens = sum_options(
            summary
                .provider_requests
                .iter()
                .map(|request| request.prompt_tokens),
        );
        summary.completion_tokens = sum_options(
            summary
                .provider_requests
                .iter()
                .map(|request| request.completion_tokens),
        );
        summary.total_tokens = sum_options(
            summary
                .provider_requests
                .iter()
                .map(|request| request.total_tokens),
        );
        summary.visible_text_chars = summary
            .provider_requests
            .iter()
            .map(|request| request.visible_text_chars)
            .sum();
        summary.thinking_chars = summary
            .provider_requests
            .iter()
            .map(|request| request.thinking_chars)
            .sum();
        summary.tool_call_count = summary
            .provider_requests
            .iter()
            .map(|request| request.tool_call_count)
            .sum();
        summary
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestPerfSummary {
    pub request_id: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<crate::provider::ProviderBackendKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<crate::provider::ProviderReasoningLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<bool>,
    pub tool_count: usize,
    pub stream: bool,
    pub message_count: usize,
    pub serialized_request_bytes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_chunk_latency_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_time_to_first_token_millis: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tokens_per_second_milli: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    pub visible_text_chars: usize,
    pub thinking_chars: usize,
    pub tool_call_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_names: Vec<String>,
}

fn sum_options(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let mut saw_value = false;
    let mut total = 0u64;
    for value in values.flatten() {
        saw_value = true;
        total = total.saturating_add(value);
    }
    saw_value.then_some(total)
}

fn min_options(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values.flatten().min()
}

/// A data-only record of an action as known by the controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub action: Action,
    pub verified_result: Option<VerifiedActionResult>,
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision: Option<PolicyDecision>,
    /// Turn that produced this action, stamped on `push_action`.
    #[serde(default)]
    pub turn_index: u64,
}

impl ActionRecord {
    pub fn new(action: Action) -> Self {
        Self {
            action,
            verified_result: None,
            failure_reason: None,
            policy_decision: None,
            turn_index: 0,
        }
    }
}

pub type ActionState = ActionLifecycleState;

/// Inspectable latest-turn reasoning/decision trace.
///
/// This intentionally stores visible provider thinking/summaries, model
/// decisions, and runtime checks. It is not authoritative action truth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReasoningTrace {
    pub user_input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub provider_planning: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub model_decisions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_checks: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBlockRecord {
    #[serde(default)]
    pub turn_index: u64,
    pub message: String,
}

impl ReasoningTrace {
    pub fn new(user_input: impl Into<String>) -> Self {
        Self {
            user_input: user_input.into(),
            route: None,
            provider_planning: Vec::new(),
            model_decisions: Vec::new(),
            runtime_checks: Vec::new(),
        }
    }

    fn push_provider_planning(&mut self, line: impl Into<String>) {
        push_bounded_unique_line(&mut self.provider_planning, line.into());
    }

    fn push_model_decision(&mut self, line: impl Into<String>) {
        push_bounded_unique_line(&mut self.model_decisions, line.into());
    }

    fn push_runtime_check(&mut self, line: impl Into<String>) {
        push_bounded_unique_line(&mut self.runtime_checks, line.into());
    }
}

/// Bounded trace of memory facts selected or omitted for the latest provider prompt.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProviderPromptMemorySelection {
    #[serde(default)]
    pub selected: Vec<ProviderPromptMemorySelectedFact>,
    #[serde(default)]
    pub omitted: Vec<ProviderPromptMemoryOmittedFact>,
}

impl ProviderPromptMemorySelection {
    pub fn new(
        selected: Vec<ProviderPromptMemorySelectedFact>,
        omitted: Vec<ProviderPromptMemoryOmittedFact>,
    ) -> Self {
        let mut selection = Self { selected, omitted };
        selection.bound();
        selection
    }

    fn bound(&mut self) {
        trim_to_limit(
            &mut self.selected,
            PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT,
        );
        trim_to_limit(
            &mut self.omitted,
            PROVIDER_PROMPT_MEMORY_SELECTION_FACT_LIMIT,
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPromptMemorySelectedFact {
    pub kind: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,
    pub source_action_id: String,
}

impl ProviderPromptMemorySelectedFact {
    pub fn new(
        kind: impl Into<String>,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            path,
            project_root,
            source_action_id: source_action_id.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPromptMemoryOmittedFact {
    pub kind: String,
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_root: Option<PathBuf>,
    pub source_action_id: String,
    pub reason: String,
}

impl ProviderPromptMemoryOmittedFact {
    pub fn new(
        kind: impl Into<String>,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind: kind.into(),
            path,
            project_root,
            source_action_id: source_action_id.into(),
            reason: reason.into(),
        }
    }
}

/// Controller-owned memory for project-building references.
///
/// This is not provider memory. Entries are created only by controller code
/// after approved filesystem/shell actions have verified their expected
/// effects, except structured plans, which are controller proposals derived
/// from verified plan files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ProjectMemory {
    #[serde(default)]
    pub verified_folders: Vec<VerifiedFolderReference>,
    #[serde(default)]
    pub verified_plans: Vec<VerifiedPlanReference>,
    #[serde(default)]
    pub structured_plans: Vec<StructuredProjectPlan>,
}

impl ProjectMemory {
    pub fn latest_verified_folder(&self) -> Option<&VerifiedFolderReference> {
        self.verified_folders.last()
    }

    pub fn latest_verified_plan(&self) -> Option<&VerifiedPlanReference> {
        self.verified_plans.last()
    }

    pub fn latest_structured_plan(&self) -> Option<&StructuredProjectPlan> {
        self.structured_plans.last()
    }

    pub fn latest_executed_structured_plan(&self) -> Option<&StructuredProjectPlan> {
        self.structured_plans
            .iter()
            .rev()
            .find(|plan| plan.runtime_status() == StructuredProjectPlanStatus::Completed)
    }

    fn remember_verified_folder(&mut self, reference: VerifiedFolderReference) {
        self.verified_folders
            .retain(|existing| existing.path != reference.path);
        self.verified_folders.push(reference);
        trim_to_memory_limit(&mut self.verified_folders);
    }

    fn remember_verified_plan(&mut self, reference: VerifiedPlanReference) {
        self.verified_plans
            .retain(|existing| existing.path != reference.path);
        self.verified_plans.push(reference);
        trim_to_memory_limit(&mut self.verified_plans);
    }

    fn remember_structured_plan(&mut self, plan: StructuredProjectPlan) {
        self.structured_plans
            .retain(|existing| existing.source_plan_path != plan.source_plan_path);
        self.structured_plans.push(plan);
        trim_to_memory_limit(&mut self.structured_plans);
    }

    fn mark_structured_plan_executed(&mut self, action_id: &str) {
        if let Some(plan) = self
            .structured_plans
            .iter_mut()
            .rev()
            .find(|plan| plan.source_action_id.as_deref() == Some(action_id))
        {
            plan.status = StructuredProjectPlanStatus::Completed;
        }
    }

    fn mark_latest_structured_plan_status(&mut self, status: StructuredProjectPlanStatus) {
        if let Some(plan) = self.structured_plans.last_mut() {
            plan.status = status;
        }
    }

    fn remove_structured_plan_for_action(&mut self, action_id: &str) {
        self.structured_plans
            .retain(|plan| plan.source_action_id.as_deref() != Some(action_id));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedFolderReference {
    pub path: PathBuf,
    pub source_action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPlanReference {
    pub path: PathBuf,
    pub project_root: PathBuf,
    pub source_action_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredProjectPlan {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action_id: Option<String>,
    pub source_plan_path: PathBuf,
    pub project_root: PathBuf,
    pub stage: String,
    #[serde(default)]
    pub status: StructuredProjectPlanStatus,
    pub expected_directories: Vec<PathBuf>,
    pub expected_files: Vec<PathBuf>,
}

impl StructuredProjectPlan {
    pub fn runtime_status(&self) -> StructuredProjectPlanStatus {
        if self.is_stale() {
            return StructuredProjectPlanStatus::Stale;
        }

        if self.has_expected_paths() && self.expected_paths_complete() {
            return StructuredProjectPlanStatus::Completed;
        }

        self.status
    }

    pub fn expected_directories_present_count(&self) -> usize {
        self.expected_directories
            .iter()
            .filter(|path| path.is_dir())
            .count()
    }

    pub fn expected_files_present_count(&self) -> usize {
        self.expected_files
            .iter()
            .filter(|path| path.is_file())
            .count()
    }

    fn is_stale(&self) -> bool {
        !self.source_plan_path.is_file()
            || !self.project_root.is_dir()
            || (self.status == StructuredProjectPlanStatus::Completed
                && self.has_expected_paths()
                && !self.expected_paths_complete())
    }

    fn has_expected_paths(&self) -> bool {
        !self.expected_directories.is_empty() || !self.expected_files.is_empty()
    }

    fn expected_paths_complete(&self) -> bool {
        self.expected_directories.iter().all(|path| path.is_dir())
            && self.expected_files.iter().all(|path| path.is_file())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StructuredProjectPlanStatus {
    Draft,
    #[default]
    #[serde(alias = "Proposed")]
    Verified,
    Executing,
    #[serde(alias = "Executed")]
    Completed,
    Stale,
}

fn trim_to_memory_limit<T>(items: &mut Vec<T>) {
    trim_to_limit(items, PROJECT_MEMORY_LIMIT);
}

fn trim_to_limit<T>(items: &mut Vec<T>, limit: usize) {
    if items.len() > limit {
        let overflow = items.len() - limit;
        items.drain(0..overflow);
    }
}

fn push_bounded_unique_line(lines: &mut Vec<String>, line: String) {
    let line = line.trim();
    if line.is_empty() || lines.iter().any(|existing| existing == line) {
        return;
    }
    lines.push(line.to_string());
    trim_to_memory_limit(lines);
}

/// Deterministic result of selecting a pending action from a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingActionSelection {
    /// No `Proposed` action exists.
    None,
    /// Exactly one `Proposed` action exists, addressed by session action index.
    Single(usize),
    /// More than one `Proposed` action exists, so no action is selected.
    Ambiguous,
}

/// Provider configuration/request metadata recorded for inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub provider: String,
    pub model: Option<String>,
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<ProviderMetrics>,
}

impl ProviderMetadata {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: None,
            request_id: None,
            metrics: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use crate::action::{Action, ActionLifecycleState};
    use crate::event::{
        ActionKind, AssistantMessage, AssistantMessageSource, Event, ProviderFinished,
        ProviderMetrics, ProviderOutput, ProviderStarted, ProviderTokenUsage, VerifiedActionResult,
    };

    use crate::plan_contract::{PlanContract, PlanContractStatus};

    use super::{
        ActionRecord, PendingActionSelection, ProjectMemory, ProviderMetadata, Session,
        StructuredProjectPlan, StructuredProjectPlanStatus, PROJECT_MEMORY_LIMIT,
    };

    #[test]
    fn new_session_stores_identity_paths_and_empty_state() {
        let session = Session::new("session-1", "/repo", "/repo/crates");

        assert_eq!(session.id, "session-1");
        assert_eq!(session.project_root, PathBuf::from("/repo"));
        assert_eq!(session.cwd, PathBuf::from("/repo/crates"));
        assert!(session.events.is_empty());
        assert!(session.actions.is_empty());
        assert_eq!(session.provider_metadata, None);
        assert_eq!(session.project_memory, ProjectMemory::default());
        assert!(session.plan_contracts.is_empty());

        let debug = format!("{session:?}");
        assert!(debug.contains("session-1"));
        assert!(debug.contains("project_root"));
    }

    #[test]
    fn structured_plan_runtime_status_tracks_verified_completed_and_stale() {
        let root =
            std::env::temp_dir().join(format!("elgar-session-plan-status-{}", std::process::id()));
        let project = root.join("DemoApp");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();

        let mut plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path.clone(),
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::default(),
            expected_directories: vec![project.join("src")],
            expected_files: vec![
                project.join("src/main.py"),
                project.join("requirements.txt"),
            ],
        };

        assert_eq!(plan.runtime_status(), StructuredProjectPlanStatus::Verified);
        assert_eq!(plan.expected_directories_present_count(), 0);
        assert_eq!(plan.expected_files_present_count(), 0);

        plan.status = StructuredProjectPlanStatus::Executing;
        assert_eq!(
            plan.runtime_status(),
            StructuredProjectPlanStatus::Executing
        );

        fs::create_dir_all(project.join("src")).unwrap();
        fs::write(project.join("src/main.py"), "print('hello')\n").unwrap();
        fs::write(project.join("requirements.txt"), "").unwrap();
        assert_eq!(
            plan.runtime_status(),
            StructuredProjectPlanStatus::Completed
        );
        assert_eq!(plan.expected_directories_present_count(), 1);
        assert_eq!(plan.expected_files_present_count(), 2);

        plan.status = StructuredProjectPlanStatus::Completed;
        fs::remove_file(project.join("requirements.txt")).unwrap();
        assert_eq!(plan.runtime_status(), StructuredProjectPlanStatus::Stale);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_records_bounded_first_class_plan_contracts() {
        let root = std::env::temp_dir().join(format!(
            "elgar-session-plan-contracts-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("demo")).unwrap();
        let mut session = Session::new("session-plan-contracts", &root, &root);

        for index in 0..(PROJECT_MEMORY_LIMIT + 2) {
            let project = root.join(format!("demo-{index}"));
            fs::create_dir_all(&project).unwrap();
            let plan_path = project.join("plan.md");
            fs::write(&plan_path, "# Plan\n").unwrap();
            let plan = StructuredProjectPlan {
                source_action_id: Some(format!("action-{index}")),
                source_plan_path: plan_path,
                project_root: project.clone(),
                stage: "verified-plan".to_string(),
                status: StructuredProjectPlanStatus::Verified,
                expected_directories: vec![project.join("src")],
                expected_files: vec![project.join("src/main.py")],
            };
            let mut contract =
                PlanContract::draft_from_structured_plan(format!("contract-{index}"), &plan);
            if index == PROJECT_MEMORY_LIMIT + 1 {
                contract.approve("user", "2026-05-28T12:00:00Z");
            }
            session.record_plan_contract(contract);
        }

        assert_eq!(session.plan_contracts().len(), PROJECT_MEMORY_LIMIT);
        assert_eq!(
            session
                .plan_contracts()
                .first()
                .map(|contract| contract.id.as_str()),
            Some("contract-2")
        );
        assert_eq!(
            session
                .latest_plan_contract()
                .map(|contract| contract.id.as_str()),
            Some("contract-9")
        );
        assert_eq!(
            session
                .latest_plan_contract()
                .map(PlanContract::runtime_status),
            Some(PlanContractStatus::Approved)
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn recording_structured_plan_creates_draft_plan_contract() {
        let root = std::env::temp_dir().join(format!(
            "elgar-session-plan-contract-draft-{}",
            std::process::id()
        ));
        let project = root.join("demo");
        let plan_path = project.join("plan.md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&project).unwrap();
        fs::write(&plan_path, "# Plan\n").unwrap();
        let mut session = Session::new("session-plan-contract-draft", &root, &root);
        let plan = StructuredProjectPlan {
            source_action_id: Some("action-plan".to_string()),
            source_plan_path: plan_path.clone(),
            project_root: project.clone(),
            stage: "verified-plan".to_string(),
            status: StructuredProjectPlanStatus::Verified,
            expected_directories: vec![project.join("src")],
            expected_files: vec![project.join("src/main.py")],
        };

        session.record_structured_project_plan(plan);

        let contract = session
            .latest_plan_contract()
            .expect("structured plan should create a draft contract");
        assert_eq!(contract.id, "plan-contract:action-plan");
        assert_eq!(contract.status, PlanContractStatus::Draft);
        assert_eq!(contract.source_plan_path, plan_path);
        assert_eq!(contract.project_root, project);
        assert_eq!(contract.source_action_id.as_deref(), Some("action-plan"));
        assert!(contract
            .scope
            .allowed_files
            .contains(&project.join("src/main.py")));
        assert!(session.project_memory().latest_structured_plan().is_some());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn session_can_hold_controller_events_action_records_and_provider_metadata() {
        let mut session = Session::new("session-2", "/repo", "/repo");

        session
            .events
            .push(Event::AssistantMessage(AssistantMessage::new(
                "I can suggest writing hello.py.",
                AssistantMessageSource::Provider,
            )));

        let mut action = ActionRecord::new(
            Action::proposed_write_file("action-1", "hello.py", "contents", "write hello.py")
                .approve()
                .mark_applied(),
        );
        action.verified_result = Some(VerifiedActionResult::FileWritten {
            path: "hello.py".to_string(),
        });
        session.actions.push(action);

        let mut provider_metadata = ProviderMetadata::new("lm-studio");
        provider_metadata.model = Some("local-model".to_string());
        provider_metadata.request_id = Some("request-1".to_string());
        session.provider_metadata = Some(provider_metadata);

        assert_eq!(session.events.len(), 1);
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Applied
        );
        assert_eq!(session.actions[0].action.kind(), ActionKind::CreateFile);
        assert_eq!(
            session.actions[0].verified_result,
            Some(VerifiedActionResult::FileWritten {
                path: "hello.py".to_string()
            })
        );
        assert_eq!(
            session
                .provider_metadata
                .as_ref()
                .map(|metadata| metadata.provider.as_str()),
            Some("lm-studio")
        );
    }

    #[test]
    fn finish_trace_turn_records_plain_turn_perf_summary() {
        let mut session = Session::new("session-perf-plain", "/repo", "/repo");
        session.start_reasoning_trace("hello");
        session.record_reasoning_route("chat");

        session.push_event(Event::ProviderStarted(
            ProviderStarted::new("lm-studio", "request-1").with_request_details(
                Some("qwen".to_string()),
                "plain_chat",
                0,
            ),
        ));
        let mut metrics =
            ProviderMetrics::new("request-1", Some("qwen".to_string()), false, 3, 1094);
        metrics.total_duration_millis = Some(12_889);
        metrics.usage = Some(ProviderTokenUsage {
            prompt_tokens: Some(233),
            completion_tokens: Some(568),
            total_tokens: Some(801),
        });
        session.push_event(Event::ProviderFinished(ProviderFinished::new(
            "lm-studio",
            "request-1",
            ProviderOutput::new("Hello!")
                .with_thinking("thinking")
                .with_metrics(metrics),
        )));

        session.finish_trace_turn();

        let summary = session
            .latest_turn_perf_summary()
            .expect("perf summary should be recorded");
        assert_eq!(summary.route.as_deref(), Some("chat"));
        assert_eq!(summary.provider_request_count, 1);
        assert_eq!(summary.request_modes, vec!["plain_chat"]);
        assert_eq!(summary.total_tool_count, 0);
        assert_eq!(summary.action_count, 0);
        assert_eq!(summary.total_provider_duration_millis, Some(12_889));
        assert_eq!(summary.prompt_tokens, Some(233));
        assert_eq!(summary.completion_tokens, Some(568));
        assert_eq!(summary.total_tokens, Some(801));
        assert_eq!(summary.message_count, 3);
        assert_eq!(summary.serialized_request_bytes, 1094);
        assert_eq!(summary.visible_text_chars, 6);
        assert_eq!(summary.thinking_chars, 8);
        assert_eq!(summary.tool_call_count, 0);
    }

    #[test]
    fn finish_trace_turn_records_multi_request_tool_perf_summary() {
        let mut session = Session::new("session-perf-tool", "/repo", "/repo");
        session.start_reasoning_trace("run build");
        session.record_reasoning_route("execute");

        session.push_event(Event::ProviderStarted(
            ProviderStarted::new("lm-studio", "request-tool").with_request_details(
                Some("qwen".to_string()),
                "tool_enabled",
                3,
            ),
        ));
        let mut tool_metrics =
            ProviderMetrics::new("request-tool", Some("qwen".to_string()), false, 5, 2000);
        tool_metrics.total_duration_millis = Some(8000);
        tool_metrics.usage = Some(ProviderTokenUsage {
            prompt_tokens: Some(500),
            completion_tokens: Some(100),
            total_tokens: Some(600),
        });
        session.push_event(Event::ProviderFinished(ProviderFinished::new(
            "lm-studio",
            "request-tool",
            ProviderOutput::new("tool request").with_metrics(tool_metrics),
        )));

        session.push_event(Event::ProviderStarted(
            ProviderStarted::new("lm-studio", "request-synthesis").with_request_details(
                Some("qwen".to_string()),
                "tool_result_synthesis",
                0,
            ),
        ));
        let mut synthesis_metrics = ProviderMetrics::new(
            "request-synthesis",
            Some("qwen".to_string()),
            false,
            6,
            1800,
        );
        synthesis_metrics.total_duration_millis = Some(4000);
        synthesis_metrics.usage = Some(ProviderTokenUsage {
            prompt_tokens: Some(700),
            completion_tokens: Some(80),
            total_tokens: Some(780),
        });
        session.push_event(Event::ProviderFinished(ProviderFinished::new(
            "lm-studio",
            "request-synthesis",
            ProviderOutput::new("Build passed.").with_metrics(synthesis_metrics),
        )));

        let mut action = ActionRecord::new(
            Action::proposed_write_file("action-1", "build.log", "ok", "write build log")
                .approve()
                .mark_applied(),
        );
        action.verified_result = Some(VerifiedActionResult::FileWritten {
            path: "build.log".to_string(),
        });
        session.push_action(action);
        session.finish_trace_turn();

        let summary = session
            .latest_turn_perf_summary()
            .expect("perf summary should be recorded");
        assert_eq!(summary.route.as_deref(), Some("execute"));
        assert_eq!(summary.provider_request_count, 2);
        assert_eq!(
            summary.request_modes,
            vec!["tool_enabled", "tool_result_synthesis"]
        );
        assert_eq!(summary.total_tool_count, 3);
        assert_eq!(summary.action_count, 1);
        assert_eq!(summary.total_provider_duration_millis, Some(12_000));
        assert_eq!(summary.prompt_tokens, Some(1200));
        assert_eq!(summary.completion_tokens, Some(180));
        assert_eq!(summary.total_tokens, Some(1380));
        assert_eq!(summary.message_count, 11);
        assert_eq!(summary.serialized_request_bytes, 3800);
        assert_eq!(summary.provider_requests.len(), 2);
    }

    #[test]
    fn provider_prose_does_not_create_action_or_verified_truth() {
        let mut session = Session::new("session-3", "/repo", "/repo");
        session.provider_metadata = Some(ProviderMetadata::new("stub-provider"));
        session
            .events
            .push(Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("I wrote hello.py successfully."),
            )));

        assert!(session.actions.is_empty());
        assert!(session
            .actions
            .iter()
            .all(|action| action.verified_result.is_none()));
    }

    #[test]
    fn provider_prose_does_not_advance_existing_action_state() {
        let mut session = Session::new("session-4", "/repo", "/repo");
        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-1",
                "hello.py",
                "contents",
                "write hello.py",
            )));

        session
            .events
            .push(Event::ProviderFinished(ProviderFinished::new(
                "stub-provider",
                "request-1",
                ProviderOutput::new("Approved and wrote hello.py."),
            )));

        assert_eq!(session.actions.len(), 1);
        assert_eq!(
            session.actions[0].action.state,
            ActionLifecycleState::Proposed
        );
        assert_eq!(session.actions[0].verified_result, None);
    }

    #[test]
    fn pending_action_selection_is_explicit_for_zero_one_and_multiple_proposed_actions() {
        let mut session = Session::new("session-5", "/repo", "/repo");

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::None
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-1",
                "first.py",
                "",
                "write first.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Single(0)
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-2",
                "second.py",
                "",
                "write second.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Ambiguous
        );
    }

    #[test]
    fn pending_action_selection_ignores_non_proposed_terminal_states() {
        let mut session = Session::new("session-6", "/repo", "/repo");
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-1", "approved.py", "", "write approved.py")
                .approve(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-2", "applied.py", "", "write applied.py")
                .approve()
                .mark_applied(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-3", "rejected.py", "", "write rejected.py")
                .reject(),
        ));
        session.actions.push(ActionRecord::new(
            Action::proposed_write_file("action-4", "failed.py", "", "write failed.py")
                .mark_failed(),
        ));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::None
        );

        session
            .actions
            .push(ActionRecord::new(Action::proposed_write_file(
                "action-5",
                "pending.py",
                "",
                "write pending.py",
            )));

        assert_eq!(
            session.pending_action_selection(),
            PendingActionSelection::Single(4)
        );
    }
}
