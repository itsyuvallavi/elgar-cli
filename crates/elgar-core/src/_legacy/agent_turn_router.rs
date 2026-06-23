use std::{
    env,
    path::{Component, Path, PathBuf},
};

use crate::{
    agent_path_utils::{absolute_session_path, normalize_path, path_is_within},
    agent_visibility::looks_like_raw_tool_protocol,
    session::{PendingActionSelection, Session, StructuredProjectPlanStatus},
    verified_state_answer::VerifiedStateAnswerKind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlainAgentChatOutcome {
    Finished,
    Execute(AgentExecutionIntent),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AgentExecutionIntent {
    pub(crate) plan_execution: bool,
    pub(crate) plan_creation_execution: bool,
    pub(crate) shell_execution: bool,
    pub(crate) after_plan_creation_decision: bool,
    pub(crate) explicit_tool_command: bool,
}

impl AgentExecutionIntent {
    pub(crate) fn is_plan_work(self) -> bool {
        self.plan_execution || self.plan_creation_execution
    }
}

pub(crate) fn state_answer_kind_can_mask_plan_execution_followup(
    kind: VerifiedStateAnswerKind,
) -> bool {
    matches!(
        kind,
        VerifiedStateAnswerKind::Pending
            | VerifiedStateAnswerKind::Status
            | VerifiedStateAnswerKind::Summary
            | VerifiedStateAnswerKind::PlanStatus
    )
}

pub(crate) fn latest_structured_plan_has_missing_paths(session: &Session) -> bool {
    session
        .project_memory()
        .latest_structured_plan()
        .is_some_and(|plan| plan.runtime_status() != StructuredProjectPlanStatus::Completed)
}

pub(crate) fn looks_like_misrouted_artifact_chat(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 1000 && path_count >= 3)
        || (path_count >= 3 && numbered_artifact_line_count(trimmed) >= 4)
}

pub(crate) fn looks_like_misrouted_artifact_chat_after_retry(content: &str) -> bool {
    let trimmed = content.trim_start();
    let path_count = local_path_like_token_count(trimmed);
    ((trimmed.starts_with('{') || trimmed.starts_with('[')) && path_count >= 2)
        || (trimmed.len() > 500 && path_count >= 3)
        || (path_count >= 3 && numbered_artifact_line_count(trimmed) >= 4)
}

pub(crate) fn looks_like_local_work_chat_misroute(input: &str, content: &str) -> bool {
    !content.trim().is_empty()
        && !looks_like_raw_tool_protocol(content)
        && !content_echoes_original_input(input, content)
        && input_contains_local_work_syntax(input)
}

pub(crate) fn classifier_chat_content_is_bad(input: &str, content: &str) -> bool {
    let content = content.trim();
    if content.is_empty() {
        return false;
    }
    content_echoes_original_input(input, content)
        || content_mentions_classifier_instructions(content)
}

fn content_mentions_classifier_instructions(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    [
        "classify",
        "classifier",
        "routing",
        "route json",
        "compact json",
        "system instructions",
        "request mode",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

pub(crate) fn route_failure_can_fall_back_to_chat(input: &str, content: &str) -> bool {
    !content.trim().is_empty()
        && !looks_like_raw_tool_protocol(content)
        && looks_like_single_non_artifact_fenced_code_block(content)
        && !looks_like_misrouted_artifact_chat(content)
        && !looks_like_misrouted_artifact_chat_after_retry(content)
        && !input_contains_local_work_syntax(input)
}

fn looks_like_single_non_artifact_fenced_code_block(content: &str) -> bool {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") || local_path_like_token_count(trimmed) > 0 {
        return false;
    }
    let mut lines = trimmed.lines();
    lines
        .next()
        .is_some_and(|line| line.trim_start().starts_with("```"))
        && trimmed
            .lines()
            .last()
            .is_some_and(|line| line.trim() == "```")
}

fn content_echoes_original_input(input: &str, content: &str) -> bool {
    let input = input.trim();
    if input.is_empty() {
        return false;
    }
    let normalized_input = normalize_echo_text(input);
    let normalized_content = normalize_echo_text(content);
    !normalized_input.is_empty() && normalized_content == normalized_input
}

fn normalize_echo_text(text: &str) -> String {
    text.trim()
        .trim_matches(|character: char| {
            character.is_ascii_punctuation() || character.is_whitespace()
        })
        .to_ascii_lowercase()
}

pub(crate) fn input_contains_local_work_syntax(input: &str) -> bool {
    local_path_like_token_count(input) > 0 || shell_syntax_token_count(input) > 0
}

pub(crate) fn input_contains_executable_command_shape(input: &str) -> bool {
    local_path_like_token_count(input) > 0
        && input
            .lines()
            .flat_map(command_shape_segments)
            .any(segment_starts_with_executable_command_shape)
}

fn command_shape_segments(line: &str) -> Vec<&str> {
    line.split([';', '|'])
        .flat_map(|segment| segment.split("&&"))
        .collect()
}

fn segment_starts_with_executable_command_shape(segment: &str) -> bool {
    for token in segment.split_whitespace() {
        if is_command_shape_env_assignment(token) {
            continue;
        }
        let Some(token) = clean_command_shape_token(token) else {
            return false;
        };
        return executable_token_exists_on_path(token);
    }
    false
}

pub(crate) fn input_has_run_prefixed_command_shape(input: &str) -> bool {
    if input.contains('?') {
        return false;
    }
    input
        .lines()
        .flat_map(command_shape_segments)
        .any(segment_has_run_prefixed_command_shape)
}

fn segment_has_run_prefixed_command_shape(segment: &str) -> bool {
    let tokens = segment
        .split_whitespace()
        .filter_map(clean_command_shape_token)
        .collect::<Vec<_>>();
    matches!(tokens.as_slice(), [first, _command, _arg, ..] if first.eq_ignore_ascii_case("run"))
}

fn is_command_shape_env_assignment(token: &str) -> bool {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

fn clean_command_shape_token(token: &str) -> Option<&str> {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';'
        )
    });
    if token.is_empty()
        || token.contains('/')
        || token.contains('=')
        || token.starts_with('-')
        || token.contains('.')
        || !token
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }
    Some(token)
}

fn executable_token_exists_on_path(token: &str) -> bool {
    let Some(path_var) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path_var).any(|dir| dir.join(token).is_file())
}

pub(crate) fn local_path_like_token_count(content: &str) -> usize {
    let mut paths = Vec::<String>::new();
    for line in content.lines() {
        let line = line.trim().trim_start_matches(|ch: char| {
            matches!(
                ch,
                '-' | '*' | '+' | '|' | '`' | '"' | '\'' | '[' | ']' | '(' | '├' | '└' | '│' | '─'
            ) || ch.is_ascii_digit()
                || ch == '.'
                || ch.is_whitespace()
        });
        for token in line
            .split(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '|' | ',' | ';' | ':' | ')' | '(' | '[' | ']' | '"' | '\'' | '{' | '}'
                    )
            })
            .filter(|part| !part.is_empty())
        {
            let token = token
                .trim_start_matches(|ch: char| {
                    matches!(ch, '-' | '*' | '+' | '├' | '└' | '│' | '─')
                })
                .trim_matches('`')
                .trim_end_matches('/');
            if token.is_empty()
                || token.contains("://")
                || token.starts_with("//")
                || token.contains('=')
                || token.starts_with('$')
                || token.starts_with('~')
            {
                continue;
            }
            let path = Path::new(token);
            let path_like = token.contains('/')
                || path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with('.')
                            || path
                                .extension()
                                .and_then(|extension| extension.to_str())
                                .is_some_and(|extension| {
                                    !extension.is_empty()
                                        && extension.len() <= 12
                                        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
                                })
                    });
            if path_like && !paths.iter().any(|seen| seen == token) {
                paths.push(token.to_string());
            }
        }
    }
    paths.len()
}

fn shell_syntax_token_count(content: &str) -> usize {
    content
        .split_whitespace()
        .filter(|token| {
            let token = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
                )
            });
            is_shell_option_token(token) || is_env_assignment_token(token)
        })
        .count()
}

fn is_shell_option_token(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('-') else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn is_env_assignment_token(token: &str) -> bool {
    let Some((name, value)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !value.is_empty()
        && !name.starts_with(|ch: char| ch.is_ascii_digit())
        && name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

pub(crate) fn explicit_project_root_from_input(session: &Session, input: &str) -> Option<PathBuf> {
    input
        .split_whitespace()
        .find_map(|token| explicit_project_root_token(session, token))
}

pub(crate) fn explicit_project_root_token(session: &Session, token: &str) -> Option<PathBuf> {
    let token = token.trim_matches(|ch: char| {
        matches!(
            ch,
            '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '.' | ':' | ';'
        )
    });
    if token.is_empty()
        || token.contains('*')
        || token.contains('?')
        || token.contains(':')
        || !(token.contains('/') || token.starts_with('/'))
    {
        return None;
    }

    let path = Path::new(token);
    if path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains('.'))
    {
        return None;
    }
    if !path.is_absolute()
        && path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
    {
        return None;
    }

    let root = normalize_path(absolute_session_path(session, path));
    (root != session.cwd && path_is_within(&root, &session.project_root)).then_some(root)
}

pub(crate) fn numbered_artifact_line_count(content: &str) -> usize {
    content
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            let digit_count = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
            digit_count > 0
                && trimmed[digit_count..]
                    .chars()
                    .next()
                    .is_some_and(|ch| matches!(ch, '.' | ')' | ':' | '-'))
        })
        .count()
}

pub(crate) fn has_verified_session_state(session: &Session) -> bool {
    session
        .actions()
        .iter()
        .any(|record| record.verified_result.is_some())
        || !matches!(
            session.pending_action_selection(),
            PendingActionSelection::None
        )
        || session.project_memory().latest_verified_folder().is_some()
        || session.project_memory().latest_verified_plan().is_some()
        || session.project_memory().latest_structured_plan().is_some()
        || session.latest_plan_contract().is_some()
}
