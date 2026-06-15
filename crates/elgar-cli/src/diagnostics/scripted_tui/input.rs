//! Scripted stdin framing for diagnostic TUI runs.
//!
//! Normal scripted TUI input remains line-based. A `/prompt` block lets dogfood
//! scripts submit one multiline prompt without changing interactive TUI input.

const PROMPT_START: &str = "/prompt";
const PROMPT_END: &str = "/end";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ScriptedInputAction {
    Submit(String),
    None,
}

#[derive(Debug, Default)]
pub(super) struct ScriptedInputFramer {
    prompt_lines: Option<Vec<String>>,
}

impl ScriptedInputFramer {
    pub(super) fn push_line(&mut self, line: String) -> ScriptedInputAction {
        if let Some(lines) = self.prompt_lines.as_mut() {
            if line.trim() == PROMPT_END {
                let prompt = lines.join("\n");
                self.prompt_lines = None;
                return ScriptedInputAction::Submit(prompt);
            }
            lines.push(line);
            return ScriptedInputAction::None;
        }

        if line.trim() == PROMPT_START {
            self.prompt_lines = Some(Vec::new());
            return ScriptedInputAction::None;
        }

        ScriptedInputAction::Submit(line)
    }

    pub(super) fn finish(self) -> Result<(), String> {
        if self.prompt_lines.is_some() {
            Err("Unterminated scripted /prompt block; add /end on its own line.".to_string())
        } else {
            Ok(())
        }
    }
}

pub(super) fn framed_inputs<I, S>(inputs: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut framer = ScriptedInputFramer::default();
    let mut framed = Vec::new();
    for input in inputs {
        if let ScriptedInputAction::Submit(prompt) = framer.push_line(input.as_ref().to_string()) {
            framed.push(prompt);
        }
    }
    framer.finish()?;
    Ok(framed)
}

#[cfg(test)]
mod tests {
    use super::{framed_inputs, ScriptedInputAction, ScriptedInputFramer};

    #[test]
    fn framed_inputs_preserve_line_mode_without_prompt_block() {
        let inputs = framed_inputs(["hello", "/exit"]).unwrap();

        assert_eq!(inputs, vec!["hello".to_string(), "/exit".to_string()]);
    }

    #[test]
    fn framed_inputs_join_prompt_blocks_as_one_submission() {
        let inputs = framed_inputs(["/prompt", "hello", "world", "/end", "/exit"]).unwrap();

        assert_eq!(
            inputs,
            vec!["hello\nworld".to_string(), "/exit".to_string()]
        );
    }

    #[test]
    fn prompt_block_requires_end_marker() {
        let error = framed_inputs(["/prompt", "hello"]).unwrap_err();

        assert!(error.contains("Unterminated"));
    }

    #[test]
    fn prompt_end_only_closes_when_it_is_the_whole_line() {
        let mut framer = ScriptedInputFramer::default();

        assert_eq!(
            framer.push_line("/prompt".to_string()),
            ScriptedInputAction::None
        );
        assert_eq!(
            framer.push_line("mention /end inline".to_string()),
            ScriptedInputAction::None
        );
        assert_eq!(
            framer.push_line("/end".to_string()),
            ScriptedInputAction::Submit("mention /end inline".to_string())
        );
    }
}
