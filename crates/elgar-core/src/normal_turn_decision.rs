use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NormalTurnDecision {
    Chat { content: String },
    Execute,
    AskGuidance { question: String },
}

pub(crate) fn parse_normal_turn_decision(message: &str) -> Option<NormalTurnDecision> {
    let value = parse_json_value(message)?;
    match value.get("route").and_then(Value::as_str)? {
        "chat" => {
            let content = value.get("content").and_then(Value::as_str)?.trim();
            (!content.is_empty()).then(|| NormalTurnDecision::Chat {
                content: content.to_string(),
            })
        }
        "execute" => Some(NormalTurnDecision::Execute),
        "ask_guidance" => {
            let question = value.get("question").and_then(Value::as_str)?.trim();
            (!question.is_empty()).then(|| NormalTurnDecision::AskGuidance {
                question: question.to_string(),
            })
        }
        _ => None,
    }
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
    use super::{parse_normal_turn_decision, NormalTurnDecision};

    #[test]
    fn parses_model_selected_execute_route() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"execute\"}"),
            Some(NormalTurnDecision::Execute)
        );
    }

    #[test]
    fn parses_model_selected_chat_route() {
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"chat\",\"content\":\"Hello.\"}"),
            Some(NormalTurnDecision::Chat {
                content: "Hello.".to_string()
            })
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
    fn rejects_unknown_or_incomplete_decisions() {
        assert_eq!(parse_normal_turn_decision("{\"route\":\"unknown\"}"), None);
        assert_eq!(parse_normal_turn_decision("{\"route\":\"chat\"}"), None);
        assert_eq!(
            parse_normal_turn_decision("{\"route\":\"ask_guidance\"}"),
            None
        );
        assert_eq!(parse_normal_turn_decision("not json"), None);
    }
}
