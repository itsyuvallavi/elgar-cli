//! Provider message builders for the primitive harness loop.
//!
//! Decision and repair calls use compact evidence summaries so the model can
//! choose the next primitive without resending every full evidence body.

use crate::{
    harness::{
        harness_loop::{
            evidence::{
                state::{evidence_prompt_stats, EvidencePromptStats},
                summary::render_compact_evidence_for_decision,
            },
            state::{
                memory::{render_working_memory_for_prompt, HarnessWorkingMemory},
                types::Evidence,
            },
        },
        loop_decision_contract, PrimitiveToolRegistry,
    },
    provider::ChatMessage,
};

const NATIVE_TOOL_LOOP_PROMPT: &str = r#"You are Elgar.

Use the attached tools when you need verified local project evidence.
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
    input: &str,
) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(NATIVE_TOOL_LOOP_PROMPT),
        ChatMessage::user(input.trim()),
    ]
}

/// Build messages for the one allowed protocol-repair call.
pub(in crate::harness::harness_loop) fn repair_prompt_context(
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

    ProviderPromptContext {
        messages: vec![
            ChatMessage::system(format!(
                "{}\n\nYour previous response did not match the harness protocol. Repair only the format. Return exactly one valid decision. Do not use markdown, code fences, bullets, or explanatory prose. {} Do not explain the repair.",
                loop_decision_contract(registry),
                repair_response_rule(evidence)
            )),
            ChatMessage::user(format!(
                "Original user request:\n{}\n\nShort-term harness memory for this turn:\n{}\n\nVerified evidence summary collected so far:\n{}\n\nValidation error:\n{}\n\nInvalid response:\n{}",
                input.trim(),
                render_working_memory_for_prompt(memory),
                evidence_text,
                validation_error,
                bounded_raw_response(raw_response)
            )),
        ],
        evidence_mode,
        stats,
    }
}

fn repair_response_rule(evidence: &[Evidence]) -> &'static str {
    if evidence.is_empty() {
        "Return either natural text, a provider tool call, or valid structured JSON."
    } else {
        "Verified evidence already exists, so natural text is invalid. Return only a provider tool call, valid structured JSON tool request, or answer_now with evidence_depth."
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
