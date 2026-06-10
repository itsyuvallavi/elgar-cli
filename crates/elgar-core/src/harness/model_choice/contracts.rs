//! Fallback text/JSON contracts for primitive harness model choice.
//!
//! Native provider tool calls are the preferred protocol. These contracts keep
//! text fallback and repair prompts aligned with the primitive registry.

use crate::harness::PrimitiveToolRegistry;

/// Render the first-call model-choice contract.
pub fn model_choice_contract(registry: &PrimitiveToolRegistry) -> String {
    let tools = registry
        .tools()
        .iter()
        .filter(|tool| tool.enabled_in_stage)
        .map(|tool| {
            let limits = tool
                .limits
                .iter()
                .map(|limit| format!("  - {limit}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "- `{}`: {}\n  side_effect_level: {}\n  executable_now: {}\n  requires_permission: {}\n  input_shape: {}\n  limits:\n{}",
                tool.id.as_str(),
                tool.description,
                tool.side_effect_level.as_str(),
                tool.executable_in_stage,
                tool.requires_permission,
                tool.input_shape,
                limits
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    format!(
        r#"You are speaking to Elgar through Elgar's fallback text/JSON tool contract.

Native provider tool calls are preferred when available. Use this text format
only when the provider returns text instead of typed tool calls.

You may either answer normally, request one available primitive tool, or request
a small batch of available primitive tools.

If answering normally, use natural text.

Available primitive tools:
{tools}

Use a primitive tool when answering well requires that tool's evidence. Do not
request tools for general conversation or questions that can be answered
without harness evidence.

Prefer `structured_requests` when several independent read-only facts are
already clearly needed at the same time, such as reading multiple known files
from a listed directory. Do not batch speculative or dependent steps; if one
result determines the next path, request only the first primitive. Use
`structured_request` for one primitive request.

When the user names a path, prefer that path before inspecting broader project
context. For `list <dir>`, request `ls` on that directory. For `read <dir>`,
request `ls` on that directory first, then batch-read the visible files if the
user asked to read the directory contents. For `find README files`, one broad
`find` pattern such as `README*` is usually enough before answering no matches.

Do not invent macro tools. For project work, compose the primitive tools:
`ls`, `find`, `grep`, `read`, `bash`, `write`, and `edit`. Elgar validates every
request and returns verified evidence or an execution error.

If requesting primitive tools, return only JSON:
{{"type":"structured_request","kind":"primitive_tool_name","reason":"short reason"}}
{{"type":"structured_requests","reason":"short reason","requests":[{{"kind":"primitive_tool_name","arguments":{{}}}}]}}

Risky primitives require explicit user approval before execution. Elgar will
return verified permission evidence when approval is needed."#
    )
}

/// Render the contract for one primitive-loop decision.
pub fn loop_decision_contract(registry: &PrimitiveToolRegistry) -> String {
    let base = model_choice_contract(registry);
    format!(
        r#"{base}

Primitive loop decision mode:
- If more evidence is needed, request one primitive tool or a small batch of
  independent primitive tools using JSON.
- Prefer `structured_requests` when the next step already clearly needs
  multiple independent `read`, `ls`, `find`, or `grep` requests. Examples:
  reading several known files from one listed directory, or checking several
  known config files. Do not batch speculative follow-up work.
- For broad requests, gather enough verified evidence for a useful answer
  before answering. Do not answer from only a directory listing unless the
  listing itself is enough for the user's request.
- Prefer the user-named path over `.` when the request names a file or
  directory. Use broader project inspection only when the named path is missing
  or the user asked for a project-wide answer.
- Natural text may be a final answer.
- If more verified evidence is needed, request more primitive tools.
- You may return `answer_now` when text fallback should intentionally switch to
  final synthesis. Include `evidence_depth:"enough"` when the evidence supports
  a normal answer, or `evidence_depth:"limited"` when the answer must clearly
  state evidence limits. If evidence is insufficient, request more primitive
  tools instead of returning `answer_now`.

Valid decision response shapes:
{{"type":"structured_request","kind":"primitive_tool_name","reason":"short reason","arguments":{{}}}}
{{"type":"structured_requests","reason":"short reason","requests":[{{"kind":"primitive_tool_name","arguments":{{}}}}]}}
{{"type":"answer_now","reason":"short reason","evidence_depth":"enough"}}
natural text final answer"#
    )
}
