//! Provider message builders for the primitive harness loop.
//!
//! Native loop calls start with a stable system prompt and then continue through
//! provider tool messages. Repair calls use compact evidence summaries only
//! when text fallback output needs one bounded format repair.

use crate::{
    harness::{
        harness_loop::{
            evidence::{
                state::{evidence_prompt_stats, EvidencePromptStats},
                summary::render_compact_evidence_for_decision,
            },
            provider::session_context::{
                native_tool_loop_turn_context, render_verified_memory_for_session,
                TurnPromptContext, HISTORY_DISCLAIMER, VERIFIED_MEMORY_HEADER,
                VERIFIED_MEMORY_PRECEDENCE_RULE,
            },
            state::{
                memory::{render_working_memory_for_prompt, HarnessWorkingMemory},
                types::Evidence,
            },
        },
        loop_decision_contract, PrimitiveToolRegistry,
    },
    provider::ChatMessage,
    session::Session,
};

pub(in crate::harness::harness_loop) const NATIVE_TOOL_LOOP_PROMPT: &str = r#"You are Elgar.

Use the attached tools when you need verified local project evidence.
When the user directly asks to open/show a file, view a folder, search/look for text, inspect, create, write, edit, or run local project state, request the matching tool or permission path instead of answering from prior messages.
For user requests like "search for X in path" or "look for X in path", use the internal `grep` primitive with that query and path.
If no tool is needed, answer normally in concise terminal-friendly text.
If tool results are provided, use them as verified evidence.
Do not claim files were read, commands ran, or files changed unless tool results prove it.
Do not invent tools. Do not request permissions.
For broad project requests, inspect selectively with the available primitive tools.
When enough evidence is available, return the final answer as normal text."#;

pub(in crate::harness::harness_loop) struct ProviderPromptContext {
    pub messages: Vec<ChatMessage>,
    pub evidence_mode: &'static str,
    pub stats: EvidencePromptStats,
}

/// Build the initial native provider tool-loop conversation.
pub(in crate::harness::harness_loop) fn native_tool_loop_initial_messages(
    session: &Session,
    input: &str,
) -> TurnPromptContext {
    native_tool_loop_turn_context(session, NATIVE_TOOL_LOOP_PROMPT, input)
}

/// Build messages for the one allowed protocol-repair call.
pub(in crate::harness::harness_loop) fn repair_prompt_context(
    session: &Session,
    input: &str,
    registry: &PrimitiveToolRegistry,
    evidence: &[Evidence],
    memory: &HarnessWorkingMemory,
    validation_error: &str,
    raw_response: &str,
) -> ProviderPromptContext {
    let stats = evidence_prompt_stats(evidence);
    let evidence_text = render_compact_evidence_for_decision(evidence);
    let evidence_mode = if evidence.is_empty() {
        "none"
    } else {
        "compact"
    };

    let turn_context = native_tool_loop_turn_context(session, NATIVE_TOOL_LOOP_PROMPT, input);
    let rendered_memory = render_verified_memory_for_session(session);
    let mut messages = turn_context.messages;
    messages.pop();

    let mut system_parts = vec![
        loop_decision_contract(registry).to_string(),
        HISTORY_DISCLAIMER.to_string(),
    ];
    if !rendered_memory.text.is_empty() {
        system_parts.push(VERIFIED_MEMORY_PRECEDENCE_RULE.to_string());
        system_parts.push(format!(
            "{VERIFIED_MEMORY_HEADER}\n{}",
            rendered_memory.text
        ));
    }
    messages[0] = ChatMessage::system(format!(
        "{}\n\nYour previous response did not match the harness protocol. Repair only the format. Return exactly one valid decision. Do not use markdown, code fences, bullets, or explanatory prose. {} Do not explain the repair.",
        system_parts.join("\n\n"),
        repair_response_rule(evidence)
    ));
    messages.push(ChatMessage::user(format!(
        "Original user request:\n{}\n\nShort-term harness memory for this turn:\n{}\n\nVerified evidence summary collected so far:\n{}\n\nValidation error:\n{}\n\nInvalid response:\n{}",
        input.trim(),
        render_working_memory_for_prompt(memory),
        evidence_text,
        validation_error,
        bounded_raw_response(raw_response)
    )));

    ProviderPromptContext {
        messages,
        evidence_mode,
        stats,
    }
}

fn repair_response_rule(evidence: &[Evidence]) -> &'static str {
    if evidence.is_empty() {
        "Return either natural text, a provider tool call, or valid structured JSON."
    } else {
        "Verified evidence already exists. Return a provider tool call, valid structured JSON tool request, answer_now with evidence_depth, or final natural text."
    }
}

fn bounded_raw_response(value: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let mut preview = value.chars().take(MAX_CHARS).collect::<String>();
    if value.chars().count() > MAX_CHARS {
        preview.push_str("...");
    }
    preview
}
