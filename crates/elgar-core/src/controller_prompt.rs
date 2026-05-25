use std::path::{Path, PathBuf};

use crate::{
    context::{context_budget_tokens, ContextBundle},
    event::{AssistantMessageSource, Event},
    session::{
        ProjectMemory, ProviderPromptMemoryOmittedFact, ProviderPromptMemorySelectedFact,
        ProviderPromptMemorySelection, Session,
    },
};

const RECENT_CONVERSATION_LINE_LIMIT: usize = 8;
const RECENT_CONVERSATION_BYTE_LIMIT: usize = 1_600;
const RECENT_CONVERSATION_LINE_BYTE_LIMIT: usize = 360;
pub(crate) const VERIFIED_MEMORY_BYTE_LIMIT: usize = 1_200;
const VERIFIED_MEMORY_LINE_BYTE_LIMIT: usize = 320;
const VERIFIED_PLAN_CONTENT_BYTE_LIMIT: usize = 720;
const VERIFIED_FOLDER_MEMORY_ENTRY_LIMIT: usize = 4;

const MODEL_FIRST_TOOL_CONTRACT: &str = "Model-first tool contract selected by Elgar controller:\n- For requests to create, implement, or make project files, return create_directory/create_file tool calls for actual filesystem changes; do not answer with prose-only file contents or claim success.\n- If the user explicitly names Desktop and gives a folder or file name, that target is clear. Use create_directory/create_file; do not ask whether Desktop means the user's home Desktop directory.\n- If target, scope, verified memory, or safe next step is ambiguous, use ask_guidance with one concise question instead of guessing.\n- Multiple safe create_file/create_directory calls are allowed for multi-file project creation.\n- Shell, overwrite, patch, delete, and move are review-gated. Do not use shell commands for package installation or project setup in this flow.\n- When verified memory names a latest folder, same folder, or plan project root, target project files inside that verified folder/root.\n- When verified memory includes a latest verified plan content excerpt, use that excerpt as the plan source; do not ask what the plan contains.";

pub(crate) fn provider_prompt_with_context(session: &mut Session, input: &str) -> String {
    let max_window_tokens = session.context_accounting().max_window_tokens;
    let recent_conversation = recent_conversation_prompt(session);
    let verified_memory = verified_memory_prompt(session, input);
    let local_context_budget = context_budget_tokens(max_window_tokens).saturating_sub(
        prompt_extension_tokens(recent_conversation.as_deref(), verified_memory.as_deref()),
    );
    let bundle = ContextBundle::from_default_local_files_with_budget(
        &session.project_root,
        &session.cwd,
        max_window_tokens,
        local_context_budget,
    );
    session.set_context_accounting(bundle.accounting.clone());
    bundle.prompt_for_with_recent_conversation_and_verified_memory(
        recent_conversation.as_deref(),
        verified_memory.as_deref(),
        input,
    )
}

pub(crate) fn model_first_provider_prompt_with_context(
    session: &mut Session,
    input: &str,
) -> String {
    let prompt = provider_prompt_with_context(session, input);
    format!("{MODEL_FIRST_TOOL_CONTRACT}\n\n{prompt}")
}

fn prompt_extension_tokens(
    recent_conversation: Option<&str>,
    verified_memory: Option<&str>,
) -> u64 {
    [recent_conversation, verified_memory]
        .into_iter()
        .flatten()
        .map(|section| (section.len() as u64).div_ceil(4))
        .sum()
}

fn verified_memory_prompt(session: &mut Session, input: &str) -> Option<String> {
    let need = VerifiedMemoryNeed::from_input(input);
    if !need.any() {
        session.set_latest_provider_prompt_memory_selection(None);
        return None;
    }

    let selection = {
        let memory = session.project_memory();
        let mut selection = VerifiedMemoryPromptSelection::default();

        if need.folder {
            select_verified_folder_memory(memory, &mut selection);
        }
        if need.plan {
            select_verified_plan_memory(memory, &mut selection);
            select_structured_plan_memory(memory, &mut selection);
        }

        selection
    };

    if selection.is_empty() {
        session.set_latest_provider_prompt_memory_selection(None);
        return None;
    }

    let mut lines = Vec::new();
    let mut selected_facts = Vec::new();
    let mut omitted_facts = Vec::new();
    for item in selection
        .selected_items
        .into_iter()
        .chain(selection.omitted_items)
    {
        if push_verified_memory_line(&mut lines, item.line, item.line_byte_limit) {
            match item.fact {
                VerifiedMemoryPromptFact::Selected(fact) => selected_facts.push(fact),
                VerifiedMemoryPromptFact::Omitted(fact) => omitted_facts.push(fact),
                VerifiedMemoryPromptFact::PromptOnly => {}
            }
        }
    }

    session.set_latest_provider_prompt_memory_selection(Some(ProviderPromptMemorySelection::new(
        selected_facts,
        omitted_facts,
    )));

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

#[derive(Debug, Default)]
struct VerifiedMemoryPromptSelection {
    selected_items: Vec<VerifiedMemoryPromptItem>,
    omitted_items: Vec<VerifiedMemoryPromptItem>,
}

impl VerifiedMemoryPromptSelection {
    fn is_empty(&self) -> bool {
        self.selected_items.is_empty() && self.omitted_items.is_empty()
    }

    fn select(
        &mut self,
        line: String,
        kind: &'static str,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: String,
    ) {
        self.selected_items.push(VerifiedMemoryPromptItem {
            line,
            line_byte_limit: VERIFIED_MEMORY_LINE_BYTE_LIMIT,
            fact: VerifiedMemoryPromptFact::Selected(ProviderPromptMemorySelectedFact::new(
                kind,
                path,
                project_root,
                source_action_id,
            )),
        });
    }

    fn omit(
        &mut self,
        line: String,
        kind: &'static str,
        path: PathBuf,
        project_root: Option<PathBuf>,
        source_action_id: String,
        reason: &'static str,
    ) {
        self.omitted_items.push(VerifiedMemoryPromptItem {
            line,
            line_byte_limit: VERIFIED_MEMORY_LINE_BYTE_LIMIT,
            fact: VerifiedMemoryPromptFact::Omitted(ProviderPromptMemoryOmittedFact::new(
                kind,
                path,
                project_root,
                source_action_id,
                reason,
            )),
        });
    }

    fn prompt_only(&mut self, line: String, line_byte_limit: usize) {
        self.selected_items.push(VerifiedMemoryPromptItem {
            line,
            line_byte_limit,
            fact: VerifiedMemoryPromptFact::PromptOnly,
        });
    }
}

#[derive(Debug)]
struct VerifiedMemoryPromptItem {
    line: String,
    line_byte_limit: usize,
    fact: VerifiedMemoryPromptFact,
}

#[derive(Debug)]
enum VerifiedMemoryPromptFact {
    Selected(ProviderPromptMemorySelectedFact),
    Omitted(ProviderPromptMemoryOmittedFact),
    PromptOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VerifiedMemoryNeed {
    pub(crate) folder: bool,
    pub(crate) plan: bool,
}

impl VerifiedMemoryNeed {
    pub(crate) fn from_input(input: &str) -> Self {
        let lower = input.to_ascii_lowercase();
        let reference = contains_any(
            &lower,
            &[
                "that ",
                "this ",
                "the folder",
                "the directory",
                "the plan",
                "the project",
                "same folder",
                "same directory",
                "inside the folder you created",
                "folder you created",
                "rest of the project",
                "go ahead and make the files",
                "make the files",
                "implement the plan",
                "where is",
                "where did you put",
                "what path",
                "path did you create",
                "dont see",
                "don't see",
                "continue",
                "next step",
                "run it",
                "execute it",
            ],
        );
        let folder = reference
            && contains_any(
                &lower,
                &[
                    "folder",
                    "directory",
                    "there",
                    "where is",
                    "where did you put",
                    "what path",
                    "path did you create",
                    "dont see",
                    "don't see",
                    "same folder",
                    "same directory",
                    "inside the folder you created",
                    "folder you created",
                    "project",
                    "files",
                    "implement the plan",
                ],
            );
        let plan = reference
            && contains_any(
                &lower,
                &[
                    "plan",
                    "implement",
                    "execute",
                    "run it",
                    "continue",
                    "next step",
                    "project",
                    "make the files",
                    "rest of the project",
                ],
            );

        Self { folder, plan }
    }

    fn any(self) -> bool {
        self.folder || self.plan
    }
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn select_verified_folder_memory(
    memory: &ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(latest_reference) = memory.verified_folders.last() else {
        return;
    };

    if !latest_reference.path.is_dir() {
        selection.omit(
            format!(
                "omitted missing verified folder: {} (source action {})",
                latest_reference.path.display(),
                latest_reference.source_action_id
            ),
            "verified_folder",
            latest_reference.path.clone(),
            None,
            latest_reference.source_action_id.clone(),
            "missing",
        );
        return;
    }

    let mut selected_count = 0;
    for reference in memory.verified_folders.iter().rev() {
        if reference.path.is_dir() {
            selection.select(
                format!(
                    "verified folder: {} (source action {})",
                    reference.path.display(),
                    reference.source_action_id
                ),
                "verified_folder",
                reference.path.clone(),
                None,
                reference.source_action_id.clone(),
            );
            selected_count += 1;
            if selected_count >= VERIFIED_FOLDER_MEMORY_ENTRY_LIMIT {
                return;
            }
            continue;
        }

        selection.omit(
            format!(
                "omitted missing verified folder: {} (source action {})",
                reference.path.display(),
                reference.source_action_id
            ),
            "verified_folder",
            reference.path.clone(),
            None,
            reference.source_action_id.clone(),
            "missing",
        );
    }
}

fn select_verified_plan_memory(
    memory: &ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(reference) = memory.verified_plans.last() else {
        return;
    };

    if !reference.path.is_file() {
        selection.omit(
            format!(
                "omitted missing verified plan: {} (source action {})",
                reference.path.display(),
                reference.source_action_id
            ),
            "verified_plan",
            reference.path.clone(),
            Some(reference.project_root.clone()),
            reference.source_action_id.clone(),
            "missing",
        );
        return;
    }
    if !reference.project_root.is_dir() {
        selection.omit(
            format!(
                "omitted verified plan with missing project root: {} -> {} (source action {})",
                reference.path.display(),
                reference.project_root.display(),
                reference.source_action_id
            ),
            "verified_plan",
            reference.path.clone(),
            Some(reference.project_root.clone()),
            reference.source_action_id.clone(),
            "missing",
        );
        return;
    }

    selection.select(
        format!(
            "latest verified plan: {} (project root {}; source action {})",
            reference.path.display(),
            reference.project_root.display(),
            reference.source_action_id
        ),
        "verified_plan",
        reference.path.clone(),
        Some(reference.project_root.clone()),
        reference.source_action_id.clone(),
    );

    if let Some(excerpt) = verified_plan_content_excerpt(&reference.path) {
        selection.prompt_only(
            format!("latest verified plan content excerpt: {excerpt}"),
            VERIFIED_PLAN_CONTENT_BYTE_LIMIT,
        );
    }
}

fn verified_plan_content_excerpt(path: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    let excerpt = compact_prompt_text(&contents);
    (!excerpt.is_empty()).then_some(excerpt)
}

fn select_structured_plan_memory(
    memory: &ProjectMemory,
    selection: &mut VerifiedMemoryPromptSelection,
) {
    let Some(plan) = memory.structured_plans.last() else {
        return;
    };
    let source_action = plan.source_action_id.as_deref().unwrap_or("(none)");
    if !plan.source_plan_path.is_file() {
        selection.omit(
            format!(
                "omitted structured plan with missing plan file: {} (source action {})",
                plan.source_plan_path.display(),
                source_action
            ),
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            source_action.to_string(),
            "missing",
        );
        return;
    }
    if !plan.project_root.is_dir() {
        selection.omit(
            format!(
                "omitted structured plan with missing project root: {} -> {} (source action {})",
                plan.source_plan_path.display(),
                plan.project_root.display(),
                source_action
            ),
            "structured_plan",
            plan.source_plan_path.clone(),
            Some(plan.project_root.clone()),
            source_action.to_string(),
            "missing",
        );
        return;
    }

    selection.select(
        format!(
            "latest structured plan: status {:?}, stage {}, expected dirs {}, expected files {}, plan {}, project root {}, source action {}",
            plan.status,
            plan.stage,
            plan.expected_directories.len(),
            plan.expected_files.len(),
            plan.source_plan_path.display(),
            plan.project_root.display(),
            source_action
        ),
        "structured_plan",
        plan.source_plan_path.clone(),
        Some(plan.project_root.clone()),
        source_action.to_string(),
    );
}

fn push_verified_memory_line(
    lines: &mut Vec<String>,
    line: String,
    line_byte_limit: usize,
) -> bool {
    let line = truncate_line(&line, line_byte_limit);
    let current_bytes = conversation_bytes(lines);
    let line_bytes = line.len() + 1;
    if current_bytes + line_bytes <= VERIFIED_MEMORY_BYTE_LIMIT {
        lines.push(line);
        true
    } else if lines.is_empty() {
        lines.push(truncate_line(&line, VERIFIED_MEMORY_BYTE_LIMIT));
        true
    } else {
        false
    }
}

fn recent_conversation_prompt(session: &Session) -> Option<String> {
    let events = session.events();
    let events = match events.last() {
        Some(Event::UserMessage(_)) => &events[..events.len().saturating_sub(1)],
        _ => events,
    };

    let mut lines = events
        .iter()
        .filter_map(recent_conversation_line)
        .map(|line| truncate_line(&line, RECENT_CONVERSATION_LINE_BYTE_LIMIT))
        .collect::<Vec<_>>();

    if lines.is_empty() {
        return None;
    }

    if lines.len() > RECENT_CONVERSATION_LINE_LIMIT {
        lines = lines[lines.len() - RECENT_CONVERSATION_LINE_LIMIT..].to_vec();
    }

    while conversation_bytes(&lines) > RECENT_CONVERSATION_BYTE_LIMIT && lines.len() > 1 {
        lines.remove(0);
    }

    if conversation_bytes(&lines) > RECENT_CONVERSATION_BYTE_LIMIT {
        lines[0] = truncate_line(&lines[0], RECENT_CONVERSATION_BYTE_LIMIT);
    }

    Some(lines.join("\n"))
}

fn recent_conversation_line(event: &Event) -> Option<String> {
    match event {
        Event::UserMessage(user) => Some(format!("user: {}", compact_prompt_text(&user.content))),
        Event::AssistantMessage(message) => Some(format!(
            "assistant({}): {}",
            assistant_source_label(message.source),
            compact_prompt_text(&message.content)
        )),
        Event::ActionProposed(action) => Some(format!(
            "controller action proposed: {:?} {} - {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)"),
            compact_prompt_text(&action.summary)
        )),
        Event::ActionApproved(action) => Some(format!(
            "controller action approved: {:?} {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)")
        )),
        Event::ActionRejected(action) => Some(format!(
            "controller action rejected: {:?} {}",
            action.action_kind,
            action.target.as_deref().unwrap_or("(no target)")
        )),
        Event::ActionApplied(action) => Some(format!(
            "controller verified action applied: {:?} {:?}",
            action.action_kind, action.result
        )),
        Event::ActionFailed(action) => Some(format!(
            "controller action failed: {:?} - {}",
            action.action_kind,
            compact_prompt_text(&action.reason)
        )),
        Event::Error(error) => Some(format!(
            "controller error: {}",
            compact_prompt_text(&error.message)
        )),
        Event::ProviderStarted(_) | Event::ProviderFinished(_) => None,
    }
}

fn assistant_source_label(source: AssistantMessageSource) -> &'static str {
    match source {
        AssistantMessageSource::Controller => "controller",
        AssistantMessageSource::Provider => "provider",
    }
}

fn compact_prompt_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn conversation_bytes(lines: &[String]) -> usize {
    lines.iter().map(|line| line.len() + 1).sum()
}

fn truncate_line(line: &str, max_bytes: usize) -> String {
    if line.len() <= max_bytes {
        return line.to_string();
    }

    let suffix = "...";
    let max_content = max_bytes.saturating_sub(suffix.len());
    let mut end = max_content.min(line.len());
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &line[..end], suffix)
}
