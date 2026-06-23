//! Streaming response helpers for OpenAI-compatible LM Studio requests.

use crate::{
    event::ProviderOutput,
    provider::{
        lm_studio::parse::{
            is_chat_stream_done_line, parse_chat_stream_line, parse_chat_stream_usage_line,
        },
        types::{ChatToolCall, ChatToolCallFunction, ProviderError, ProviderStreamChunk},
    },
    token_accounting::ProviderTokenUsage,
};

pub(super) fn emit_output_chunks(
    output: &ProviderOutput,
    on_chunk: &mut dyn FnMut(ProviderStreamChunk),
) {
    if let Some(thinking) = output.thinking.as_ref() {
        on_chunk(ProviderStreamChunk::Reasoning(thinking.clone()));
    }
    on_chunk(ProviderStreamChunk::Text(output.text.clone()));
}

#[derive(Debug, Default)]
pub(super) struct StreamingOutputParts {
    pub(super) text: String,
    pub(super) thinking: String,
    pub(super) tool_calls: Vec<StreamingToolCallParts>,
    usage: Option<ProviderTokenUsage>,
    pending_line: String,
    done_received: bool,
}

impl StreamingOutputParts {
    pub(super) fn push_body_chunk(
        &mut self,
        body_chunk: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<bool, ProviderError> {
        self.pending_line.push_str(body_chunk);
        while let Some(newline) = self.pending_line.find('\n') {
            let mut line = self.pending_line[..newline].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            self.pending_line.drain(..=newline);
            self.push_line(&line, on_chunk)?;
            if self.done_received {
                self.pending_line.clear();
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn finish(
        &mut self,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        if !self.pending_line.trim().is_empty() {
            let line = std::mem::take(&mut self.pending_line);
            self.push_line(line.trim_end_matches('\r'), on_chunk)?;
        }
        Ok(())
    }

    fn push_line(
        &mut self,
        line: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        if let Some(usage) = parse_chat_stream_usage_line(line)? {
            self.usage = Some(usage);
        }
        if is_chat_stream_done_line(line) {
            self.done_received = true;
            return Ok(());
        }

        for chunk in parse_chat_stream_line(line)? {
            match &chunk {
                ProviderStreamChunk::Reasoning(value) => self.thinking.push_str(value),
                ProviderStreamChunk::Text(value) => self.text.push_str(value),
                ProviderStreamChunk::ToolCallDelta(delta) => {
                    while self.tool_calls.len() <= delta.index {
                        self.tool_calls.push(StreamingToolCallParts::default());
                    }
                    if let Some(call) = self.tool_calls.get_mut(delta.index) {
                        if let Some(id) = delta.id.as_ref() {
                            call.id = Some(id.clone());
                        }
                        if let Some(tool_type) = delta.tool_type.as_ref() {
                            call.tool_type = Some(tool_type.clone());
                        }
                        if let Some(name) = delta.function_name.as_ref() {
                            call.function_name.push_str(name);
                        }
                        if let Some(arguments) = delta.function_arguments.as_ref() {
                            call.function_arguments.push_str(arguments);
                        }
                    }
                }
            }
            on_chunk(chunk);
        }
        Ok(())
    }

    pub(super) fn usage(&self) -> Option<&ProviderTokenUsage> {
        self.usage.as_ref()
    }

    pub(super) fn finish_output(self) -> Result<ProviderOutput, ProviderError> {
        let tool_calls = self
            .tool_calls
            .into_iter()
            .filter_map(StreamingToolCallParts::into_tool_call)
            .collect::<Vec<_>>();

        let trimmed_text = self.text.trim().to_string();
        if trimmed_text.is_empty() && tool_calls.is_empty() {
            return Err(ProviderError::empty_response(
                "provider stream contained no text or tool calls",
            ));
        }

        let mut output = ProviderOutput::new(trimmed_text).with_tool_calls(tool_calls);
        let trimmed_thinking = self.thinking.trim();
        if !trimmed_thinking.is_empty() {
            output = output.with_thinking(trimmed_thinking.to_string());
        }
        Ok(output)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct StreamingToolCallParts {
    id: Option<String>,
    tool_type: Option<String>,
    function_name: String,
    function_arguments: String,
}

impl StreamingToolCallParts {
    fn into_tool_call(self) -> Option<ChatToolCall> {
        if self.function_name.trim().is_empty() {
            return None;
        }

        Some(ChatToolCall {
            id: self.id.unwrap_or_else(|| "streamed-tool-call".to_string()),
            tool_type: self.tool_type.unwrap_or_else(|| "function".to_string()),
            function: ChatToolCallFunction {
                name: self.function_name,
                arguments: self.function_arguments,
            },
        })
    }
}
