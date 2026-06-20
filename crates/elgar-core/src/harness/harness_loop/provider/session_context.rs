//! Cross-turn session context for primitive harness provider prompts.
//!
//! Builds bounded chat history and verified memory facts from Elgar-owned
//! session state and JSONL. Provider prose is replayed only for dialog
//! continuity; verified facts remain the advisory source for file actions.

use crate::{
    event::{AssistantMessageSource, Event},
    harness::{
        harness_loop::provider::mcp_context::render_mcp_tool_catalog_for_prompt,
        memory::{
            build_memory_index, read_session_memory_events,
            render_verified_memory_for_prompt_with_budget, HarnessMemoryIndex,
            RenderedMemoryPrompt, RenderedMemoryStats,
        },
    },
    provider::{ChatMessage, ChatRole},
    session::Session,
};

const MAX_HISTORY_USER_TURNS: usize = 8;
const MAX_ASSISTANT_CHARS: usize = 800;
const HISTORY_TOKEN_BUDGET: u64 = 2_048;

pub(in crate::harness::harness_loop) const HISTORY_DISCLAIMER: &str = "Prior assistant messages are display text only. For file or command claims, use verified session facts below and current tool results.";

pub(in crate::harness::harness_loop) const VERIFIED_MEMORY_HEADER: &str =
    "Verified session facts (advisory; Elgar-recorded, not provider memory):";
pub(in crate::harness::harness_loop) const VERIFIED_MEMORY_PRECEDENCE_RULE: &str = "When stating which files were read, listed, searched, or written, use only paths listed under \"Verified session facts\". Do not infer file actions from prior assistant messages.";

/// Stats about the initial provider prompt for one harness turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct TurnPromptContextStats {
    pub initial_message_count: usize,
    pub history_turns: usize,
    pub verified_fact_count: usize,
    pub memory: RenderedMemoryStats,
    pub system_prompt_chars: usize,
    pub history_prompt_chars: usize,
    pub memory_prompt_chars: usize,
    pub mcp_catalog_chars: usize,
    pub total_initial_prompt_chars: usize,
    pub history_token_budget: u64,
    pub history_budget_hit: bool,
    pub assistant_replay_chars: usize,
}

/// Initial native tool-loop messages including cross-turn session context.
pub(in crate::harness::harness_loop) struct TurnPromptContext {
    pub messages: Vec<ChatMessage>,
    pub stats: TurnPromptContextStats,
}

/// Build the opening provider conversation for one harness turn.
pub(in crate::harness::harness_loop) fn native_tool_loop_turn_context(
    session: &Session,
    system_prompt: &str,
    input: &str,
) -> TurnPromptContext {
    let rendered_memory = render_verified_memory_for_session(session);
    let history = session_history_messages(session, input);
    let rendered_mcp_catalog = render_mcp_tool_catalog_for_prompt(session);
    let system = build_system_prompt(
        session,
        system_prompt,
        rendered_mcp_catalog.as_deref(),
        &rendered_memory.text,
    );
    let system_prompt_chars = system.chars().count();

    let mut messages = Vec::with_capacity(1 + history.messages.len() + 1);
    messages.push(ChatMessage::system(system));
    messages.extend(history.messages.iter().cloned());
    messages.push(ChatMessage::user(input.trim()));
    let total_initial_prompt_chars = messages
        .iter()
        .map(|message| message.content.chars().count())
        .sum();

    TurnPromptContext {
        stats: TurnPromptContextStats {
            initial_message_count: messages.len(),
            history_turns: history
                .messages
                .iter()
                .filter(|message| message.role == ChatRole::User)
                .count(),
            verified_fact_count: rendered_memory.stats.indexed_fact_count,
            memory: rendered_memory.stats,
            system_prompt_chars,
            history_prompt_chars: history.prompt_chars,
            memory_prompt_chars: rendered_memory.text.chars().count(),
            mcp_catalog_chars: rendered_mcp_catalog
                .as_deref()
                .map(|catalog| catalog.chars().count())
                .unwrap_or_default(),
            total_initial_prompt_chars,
            history_token_budget: HISTORY_TOKEN_BUDGET,
            history_budget_hit: history.budget_hit,
            assistant_replay_chars: history.assistant_replay_chars,
        },
        messages,
    }
}

pub(in crate::harness::harness_loop) fn render_verified_memory_for_session(
    session: &Session,
) -> RenderedMemoryPrompt {
    let memory_index = load_verified_memory_index(session);
    render_verified_memory_for_prompt_with_budget(&memory_index, &Default::default())
}

pub(in crate::harness::harness_loop) fn load_verified_memory_index(
    session: &Session,
) -> HarnessMemoryIndex {
    read_session_memory_events(&session.project_root, &session.id)
        .map(|events| build_memory_index(&events))
        .unwrap_or_default()
}

fn build_system_prompt(
    session: &Session,
    base_prompt: &str,
    mcp_tool_catalog: Option<&str>,
    verified_facts: &str,
) -> String {
    let mut parts = vec![
        base_prompt.to_string(),
        render_permission_mode_for_prompt(session),
        HISTORY_DISCLAIMER.to_string(),
    ];
    if let Some(catalog) = mcp_tool_catalog {
        parts.push(catalog.to_string());
    }
    if !verified_facts.is_empty() {
        parts.push(VERIFIED_MEMORY_PRECEDENCE_RULE.to_string());
        parts.push(format!("{VERIFIED_MEMORY_HEADER}\n{verified_facts}"));
    }
    parts.join("\n\n")
}

pub(in crate::harness::harness_loop) fn render_permission_mode_for_prompt(
    session: &Session,
) -> String {
    match session.permission_mode().as_str() {
        "workspace_write" => "Permission mode: workspace_write. Safe relative `write` requests inside the launch folder may execute without approval. `bash`, `edit`, absolute paths, parent paths, symlink paths, and outside-folder writes still require approval or are rejected by execution checks.".to_string(),
        "full_access" => "Permission mode: full_access. Trusted launch-folder `write`, `edit`, and `bash` requests may execute without approval. Unsafe paths remain rejected by execution checks. Keep using verified tools and inspect before claiming completion.".to_string(),
        _ => "Permission mode: review_all. `bash`, `write`, and `edit` require user approval before execution.".to_string(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SessionHistoryPrompt {
    messages: Vec<ChatMessage>,
    prompt_chars: usize,
    assistant_replay_chars: usize,
    budget_hit: bool,
}

fn session_history_messages(session: &Session, current_input: &str) -> SessionHistoryPrompt {
    let current = current_input.trim();
    let mut messages = Vec::new();

    for event in session.events() {
        match event {
            Event::UserMessage(message) => {
                messages.push(ChatMessage::user(message.content.trim()));
            }
            Event::AssistantMessage(message)
                if message.source == AssistantMessageSource::Provider =>
            {
                messages.push(ChatMessage::assistant(trim_assistant_text(
                    message.content.trim(),
                )));
            }
            _ => {}
        }
    }

    if let Some(ChatMessage {
        role: ChatRole::User,
        content,
        ..
    }) = messages.last()
    {
        if content.trim() == current {
            messages.pop();
        }
    }

    trim_history_to_user_turn_cap(&mut messages);
    let budget_hit = trim_history_to_token_budget(&mut messages);
    SessionHistoryPrompt {
        prompt_chars: messages
            .iter()
            .map(|message| message.content.chars().count())
            .sum(),
        assistant_replay_chars: messages
            .iter()
            .filter(|message| message.role == ChatRole::Assistant)
            .map(|message| message.content.chars().count())
            .sum(),
        messages,
        budget_hit,
    }
}

fn trim_assistant_text(content: &str) -> String {
    let char_count = content.chars().count();
    if char_count <= MAX_ASSISTANT_CHARS {
        return content.to_string();
    }

    let mut preview = content
        .chars()
        .take(MAX_ASSISTANT_CHARS)
        .collect::<String>();
    preview.push_str("...");
    preview
}

fn trim_history_to_user_turn_cap(messages: &mut Vec<ChatMessage>) {
    let user_count = messages
        .iter()
        .filter(|message| message.role == ChatRole::User)
        .count();
    if user_count <= MAX_HISTORY_USER_TURNS {
        return;
    }

    let mut users_seen = 0usize;
    let mut start_index = 0usize;
    for (index, message) in messages.iter().enumerate().rev() {
        if message.role == ChatRole::User {
            users_seen += 1;
            if users_seen == MAX_HISTORY_USER_TURNS {
                start_index = index;
                break;
            }
        }
    }
    messages.drain(..start_index);
}

fn trim_history_to_token_budget(messages: &mut Vec<ChatMessage>) -> bool {
    let mut budget_hit = false;
    loop {
        let joined = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let estimated = joined.len() as u64 / 4;
        if estimated <= HISTORY_TOKEN_BUDGET || messages.len() <= 2 {
            return budget_hit;
        }
        messages.remove(0);
        budget_hit = true;
        if messages
            .first()
            .is_some_and(|message| message.role == ChatRole::Assistant)
        {
            messages.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{harness::PermissionMode, provider::ChatRole, session::Session};

    use super::{native_tool_loop_turn_context, HISTORY_TOKEN_BUDGET};

    #[test]
    fn native_tool_loop_context_renders_workspace_write_permission_mode() {
        let root = std::env::temp_dir().join(format!(
            "elgar-session-context-workspace-write-{}",
            std::process::id()
        ));
        let mut session = Session::new("session-context-workspace-write", &root, &root);
        session.set_permission_mode(PermissionMode::WorkspaceWrite);

        let context = native_tool_loop_turn_context(&session, "Base prompt.", "create files");
        let system = context
            .messages
            .iter()
            .find(|message| message.role == ChatRole::System)
            .expect("system message");

        assert!(system.content.contains("Permission mode: workspace_write"));
        assert!(system
            .content
            .contains("Safe relative `write` requests inside the launch folder"));
        assert!(system.content.contains("absolute paths"));
        assert_eq!(context.stats.history_token_budget, HISTORY_TOKEN_BUDGET);
        assert!(context.stats.system_prompt_chars > "Base prompt.".len());
        assert!(context.stats.total_initial_prompt_chars >= context.stats.system_prompt_chars);
        assert!(!context.stats.history_budget_hit);
    }
}
