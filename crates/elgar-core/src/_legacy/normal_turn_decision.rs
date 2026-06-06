use serde_json::Value;

use crate::verified_state_answer::{parse_verified_state_answer_kind, VerifiedStateAnswerKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalTurnDecision {
    Chat {
        content: Option<String>,
    },
    Execute {
        intent: Option<NormalTurnExecuteIntent>,
    },
    AskGuidance {
        question: String,
    },
    State {
        answer_kind: Option<VerifiedStateAnswerKind>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum NormalTurnExecuteIntent {
    PlanExecution,
    PlanCreationAndExecution,
    ShellExecution,
}

pub(crate) fn parse_normal_turn_decision(message: &str) -> Option<NormalTurnDecision> {
    let value = parse_json_value(message)?;
    match value.get("route").and_then(Value::as_str)? {
        "chat" => {
            let content = value
                .get("content")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|content| !content.is_empty())
                .map(ToString::to_string);
            Some(NormalTurnDecision::Chat { content })
        }
        "execute" => Some(NormalTurnDecision::Execute {
            intent: parse_execute_intent(&value),
        }),
        "ask_guidance" => {
            let question = value.get("question").and_then(Value::as_str)?.trim();
            (!question.is_empty()).then(|| NormalTurnDecision::AskGuidance {
                question: question.to_string(),
            })
        }
        "state" => Some(NormalTurnDecision::State {
            answer_kind: parse_state_answer_kind(&value),
        }),
        _ => None,
    }
}

fn parse_execute_intent(value: &Value) -> Option<NormalTurnExecuteIntent> {
    match value.get("intent").and_then(Value::as_str)?.trim() {
        "plan_execution" => Some(NormalTurnExecuteIntent::PlanExecution),
        "plan_creation_execution" => Some(NormalTurnExecuteIntent::PlanCreationAndExecution),
        "shell_execution" => Some(NormalTurnExecuteIntent::ShellExecution),
        _ => None,
    }
}

fn parse_state_answer_kind(value: &Value) -> Option<VerifiedStateAnswerKind> {
    value
        .get("answer_kind")
        .and_then(Value::as_str)
        .and_then(parse_verified_state_answer_kind)
}

fn parse_json_value(message: &str) -> Option<Value> {
    serde_json::from_str::<Value>(message.trim())
        .ok()
        .or_else(|| {
            let start = message.find('{')?;
            let end = message.rfind('}')?;
            (start < end)
                .then(|| serde_json::from_str::<Value>(&message[start..=end]).ok())
                .flatten()
        })
}

#[cfg(test)]
mod tests {
    use crate::verified_state_answer::VerifiedStateAnswerKind;

    use super::{parse_normal_turn_decision, NormalTurnDecision, NormalTurnExecuteIntent};

    #[test]
    fn parses_model_selected_execute_route() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"execute\"}"),
            Some(NormalTurnDecision::Execute { intent: None })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"execute\",\"intent\":\"plan_execution\"}"),
            Some(NormalTurnDecision::Execute {
                intent: Some(NormalTurnExecuteIntent::PlanExecution)
            })
        );
        assert_eq!(
            parse_normal_turn_decision(
                "{\"route\":\"execute\",\"intent\":\"plan_creation_execution\"}"
            ),
            Some(NormalTurnDecision::Execute {
                intent: Some(NormalTurnExecuteIntent::PlanCreationAndExecution)
            })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"execute\",\"intent\":\"shell_execution\"}"),
            Some(NormalTurnDecision::Execute {
                intent: Some(NormalTurnExecuteIntent::ShellExecution)
            })
        );
    }

    #[test]
    fn parses_model_selected_chat_route() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"chat\",\"content\":\"Hello.\"}"),
            Some(NormalTurnDecision::Chat {
                content: Some("Hello.".to_string())
            })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"chat\"}"),
            Some(NormalTurnDecision::Chat { content: None })
        );
    }

    #[test]
    fn parses_model_selected_guidance_route_from_wrapped_json() {
        assert_eq!(
            parse_normal_turn_decision(
                "```json\n{\"route\":\"ask_guidance\",\"question\":\"Which path?\"}\n```"
            ),
            Some(NormalTurnDecision::AskGuidance {
                question: "Which path?".to_string()
            })
        );
    }

    #[test]
    fn parses_model_selected_verified_state_route() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\",\"answer_kind\":\"latest_folder\"}"),
            Some(NormalTurnDecision::State {
                answer_kind: Some(VerifiedStateAnswerKind::LatestFolder)
            })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\",\"answer_kind\":\"plan_details\"}"),
            Some(NormalTurnDecision::State {
                answer_kind: Some(VerifiedStateAnswerKind::PlanDetails)
            })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\",\"answer_kind\":\"recent_changes\"}"),
            Some(NormalTurnDecision::State {
                answer_kind: Some(VerifiedStateAnswerKind::RecentChanges)
            })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\",\"answer_kind\":\"last_block\"}"),
            Some(NormalTurnDecision::State {
                answer_kind: Some(VerifiedStateAnswerKind::LastBlock)
            })
        );
    }

    #[test]
    fn state_route_without_kind_is_valid_and_defers_kind_to_runtime() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\"}"),
            Some(NormalTurnDecision::State { answer_kind: None })
        );
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"state\",\"answer_kind\":\"not_a_kind\"}"),
            Some(NormalTurnDecision::State { answer_kind: None })
        );
    }

    #[test]
    fn rejects_unknown_or_incomplete_decisions() {
        assert_eq!(parse_normal_turn_decision("{\"route\":\"unknown\"}"), None);
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"ask_guidance\"}"),
            None
        );
        assert_eq!(parse_normal_turn_decision("not json"), None);
    }
}
