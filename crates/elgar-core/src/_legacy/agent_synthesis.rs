use crate::{
    action::ShellActionVerification,
    agent_display_intent::input_requests_analysis,
    agent_request_mode::{provider_request_metadata_for_mode, AgentProviderRequestMode},
    event::{
        AssistantMessage, AssistantMessageSource, ErrorEvent, Event, ProviderFinished,
        ProviderStarted,
    },
    normal_turn_decision::parse_normal_turn_decision,
    provider::{ChatMessage, ControllerProvider},
    provider_visible_text_from_text_only_output,
    session::Session,
};

pub(crate) const AGENT_SHELL_RESULT_SYNTHESIS_PROMPT: &str = concat!(
    "You are Elgar writing the final answer after a verified shell command. ",
    "Do not call, request, or describe tools. ",
    "Use only the verified shell result supplied in this request. ",
    "If stdout or stderr is relevant, copy exact short output values character-for-character. ",
    "Answer briefly in normal prose."
);

pub(crate) fn request_explicit_shell_tool_result_synthesis<P>(
    provider: &P,
    session: &mut Session,
    messages: &[ChatMessage],
) where
    P: ControllerProvider,
{
    let request =
        provider_request_metadata_for_mode(provider, AgentProviderRequestMode::ToolResultSynthesis);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "tool_result_synthesis", 0),
    ));

    let mut synthesis_messages = messages.to_vec();
    synthesis_messages.push(ChatMessage::system(
        "The requested shell command has completed and the tool result is already in this conversation. Do not request or describe any more tool calls. Answer the user now in normal prose using the tool result.",
    ));

    match provider.chat_messages_without_streaming_with_metadata(synthesis_messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            push_synthesis_provider_message_if_visible(session, assistant_text);
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }
}

pub(crate) fn request_shell_transaction_tool_result_synthesis<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    shell: &ShellActionVerification,
    is_direct_file_read: bool,
) where
    P: ControllerProvider,
{
    let digest = verified_shell_result_digest(shell);
    let request =
        provider_request_metadata_for_mode(provider, AgentProviderRequestMode::ToolResultSynthesis);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "tool_result_synthesis", 0),
    ));

    let messages = vec![
        ChatMessage::system(AGENT_SHELL_RESULT_SYNTHESIS_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
        ChatMessage::system(
            "You are writing the final answer for a completed shell action. Use only the verified shell result below. When reporting command output values, copy exact stdout/stderr text character-for-character from stdout_exact or stderr_exact. Do not request or describe more tool calls. Do not ask the user to paste output already present in the verified result. Do not claim files were changed unless verified. Answer briefly and directly.",
        ),
        ChatMessage::system(digest.clone()),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            if synthesis_omits_short_exact_stdout(
                input,
                &assistant_text,
                shell,
                is_direct_file_read,
            ) {
                session.push_reasoning_runtime_check(
                    "tool-result synthesis omitted short exact stdout; retrying with exact-output correction",
                );
                request_shell_transaction_tool_result_synthesis_retry(
                    provider, session, input, shell, &digest,
                );
                return;
            }
            push_synthesis_provider_message_if_visible(session, assistant_text);
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }
}

pub(crate) fn push_synthesis_provider_message_if_visible(
    session: &mut Session,
    message: impl Into<String>,
) {
    let message = message.into();
    if parse_normal_turn_decision(&message).is_some() {
        session.push_reasoning_runtime_check(
            "synthesis provider returned route JSON; suppressed visible provider text",
        );
        return;
    }
    push_provider_message_if_visible(session, message);
}

pub(crate) fn verified_shell_result_digest(shell: &ShellActionVerification) -> String {
    let result_class = if shell.timed_out {
        "timeout"
    } else if shell.exit_code == Some(0) {
        "success"
    } else if shell.exit_code.is_some() {
        "failure"
    } else {
        "unknown"
    };
    let mut digest = String::new();
    digest.push_str("VERIFIED_SHELL_RESULT\n");
    digest.push_str("command: ");
    digest.push_str(shell.command.trim());
    digest.push('\n');
    digest.push_str("cwd: ");
    digest.push_str(shell.cwd.trim());
    digest.push('\n');
    digest.push_str("exit_code: ");
    digest.push_str(
        &shell
            .exit_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
    );
    digest.push('\n');
    digest.push_str("elapsed_millis: ");
    digest.push_str(&shell.elapsed_millis.to_string());
    digest.push('\n');
    digest.push_str("timed_out: ");
    digest.push_str(if shell.timed_out { "true" } else { "false" });
    digest.push('\n');
    append_shell_digest_text_section(&mut digest, "stdout_summary", &shell.stdout);
    append_shell_digest_text_section(&mut digest, "stderr_summary", &shell.stderr);
    append_shell_digest_exact_section(&mut digest, "stdout_exact", &shell.stdout);
    append_shell_digest_exact_section(&mut digest, "stderr_exact", &shell.stderr);
    digest.push_str("stdout_truncated: ");
    digest.push_str(if shell.stdout_truncated {
        "true"
    } else {
        "false"
    });
    digest.push('\n');
    digest.push_str("stderr_truncated: ");
    digest.push_str(if shell.stderr_truncated {
        "true"
    } else {
        "false"
    });
    digest.push('\n');
    if let Some(effect) = shell
        .verified_effect
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        digest.push_str("verified_effect: ");
        digest.push_str(&compact_context_line(effect));
        digest.push('\n');
    }
    digest.push_str("result_class: ");
    digest.push_str(result_class);
    digest.push('\n');
    digest.push_str("answer_now: ");
    digest.push_str(if shell.timed_out || shell.exit_code.is_some() {
        "true"
    } else {
        "false"
    });
    digest.push('\n');
    digest.push_str("raw_details_available: true");
    digest
}

fn request_shell_transaction_tool_result_synthesis_retry<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    shell: &ShellActionVerification,
    digest: &str,
) where
    P: ControllerProvider,
{
    let request =
        provider_request_metadata_for_mode(provider, AgentProviderRequestMode::ToolResultSynthesis);
    session.push_event(Event::ProviderStarted(
        ProviderStarted::new(request.provider.clone(), request.request_id.clone())
            .with_request_details(request.model.clone(), "tool_result_synthesis", 0),
    ));
    let exact_lines = short_exact_stdout_lines(shell).join("\n");
    let messages = vec![
        ChatMessage::system(AGENT_SHELL_RESULT_SYNTHESIS_PROMPT),
        ChatMessage::system(agent_route_location_context(session)),
        ChatMessage::system(
            "Your previous shell-result answer did not preserve exact verified stdout. Answer again using only the verified shell result. Copy the exact stdout line below character-for-character when reporting the command output.",
        ),
        ChatMessage::system(format!("EXACT_VERIFIED_STDOUT\n```text\n{exact_lines}\n```")),
        ChatMessage::system(digest.to_string()),
        ChatMessage::user(input),
    ];

    match provider.chat_messages_without_streaming_with_metadata(messages, &request) {
        Ok(output) => {
            let assistant_text = output.text.clone();
            push_provider_finished(session, request.provider, request.request_id, output);
            push_synthesis_provider_message_if_visible(session, assistant_text);
        }
        Err(error) => {
            session.push_event(Event::Error(ErrorEvent::new(format!(
                "{} provider request {} failed: {error}",
                request.provider, request.request_id
            ))));
        }
    }
}

fn synthesis_omits_short_exact_stdout(
    input: &str,
    assistant_text: &str,
    shell: &ShellActionVerification,
    is_direct_file_read: bool,
) -> bool {
    if is_direct_file_read && input_requests_analysis(&input.to_ascii_lowercase()) {
        return false;
    }
    let lines = short_exact_stdout_lines(shell);
    !lines.is_empty() && lines.iter().any(|line| !assistant_text.contains(line))
}

fn short_exact_stdout_lines(shell: &ShellActionVerification) -> Vec<String> {
    if shell.stdout_truncated || shell.stdout.chars().count() > 600 {
        return Vec::new();
    }
    let lines = shell
        .stdout
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if lines.len() > 3 || lines.iter().any(|line| line.chars().count() > 240) {
        return Vec::new();
    }
    lines
}

fn push_provider_finished(
    session: &mut Session,
    provider: String,
    request_id: String,
    output: crate::event::ProviderOutput,
) {
    if let Some(metrics) = output.metrics.as_ref() {
        session.record_provider_metrics(metrics);
    }
    session.push_event(Event::ProviderFinished(ProviderFinished::new(
        provider, request_id, output,
    )));
}

fn push_provider_message_if_visible(session: &mut Session, message: impl Into<String>) {
    let message = message.into();
    if let Some(message) = provider_visible_text_from_text_only_output(message) {
        session.push_event(Event::AssistantMessage(AssistantMessage::new(
            message,
            AssistantMessageSource::Provider,
        )));
    }
}

fn agent_route_location_context(session: &Session) -> String {
    format!(
        "Elgar runtime session:\nproject_root: {}\ncwd: {}\ncwd_relative_to_project_root: {}\nWhen the user refers to current/root/this folder/project, use cwd unless they explicitly name another path.",
        session.project_root.display(),
        session.cwd.display(),
        session
            .cwd
            .strip_prefix(&session.project_root)
            .ok()
            .and_then(|path| (!path.as_os_str().is_empty()).then(|| path.display().to_string()))
            .unwrap_or_else(|| ".".to_string())
    )
}

fn append_shell_digest_exact_section(digest: &mut String, label: &str, value: &str) {
    const MAX_EXACT_CHARS: usize = 1_200;
    let trimmed = value.trim_end();
    if trimmed.trim().is_empty() {
        digest.push_str(label);
        digest.push_str(": empty\n");
        return;
    }
    if trimmed.chars().count() > MAX_EXACT_CHARS {
        digest.push_str(label);
        digest.push_str(": omitted_large_output\n");
        return;
    }
    digest.push_str(label);
    digest.push_str(":\n```text\n");
    digest.push_str(trimmed);
    digest.push_str("\n```\n");
}

fn append_shell_digest_text_section(digest: &mut String, label: &str, value: &str) {
    let lines = shell_digest_excerpt_lines(value);
    if lines.is_empty() {
        digest.push_str(label);
        digest.push_str(": empty\n");
        return;
    }
    digest.push_str(label);
    digest.push_str(":\n");
    for line in lines {
        digest.push_str("- ");
        digest.push_str(&line);
        digest.push('\n');
    }
}

fn shell_digest_excerpt_lines(value: &str) -> Vec<String> {
    let cleaned = value
        .lines()
        .map(compact_context_line)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if cleaned.len() <= 6 {
        return cleaned;
    }
    let mut excerpt = Vec::new();
    excerpt.extend(cleaned.iter().take(3).cloned());
    excerpt.push("... truncated ...".to_string());
    excerpt.extend(
        cleaned
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev(),
    );
    excerpt
}

fn compact_context_line(value: &str) -> String {
    let line = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_utf8(&line, 260)
}

fn truncate_utf8(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut end = 0usize;
    for (index, _) in value.char_indices() {
        if index <= max_bytes {
            end = index;
        } else {
            break;
        }
    }
    value[..end].to_string()
}
