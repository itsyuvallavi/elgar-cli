//! Streaming response helpers for OpenAI-compatible LM Studio requests.

use crate::{
    event::ProviderOutput,
    provider::{
        lm_studio::parse::parse_chat_stream_line,
        types::{ProviderError, ProviderStreamChunk},
    },
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
    pending_line: String,
}

impl StreamingOutputParts {
    pub(super) fn push_body_chunk(
        &mut self,
        body_chunk: &str,
        on_chunk: &mut dyn FnMut(ProviderStreamChunk),
    ) -> Result<(), ProviderError> {
        self.pending_line.push_str(body_chunk);
        while let Some(newline) = self.pending_line.find('\n') {
            let mut line = self.pending_line[..newline].to_string();
            if line.ends_with('\r') {
                line.pop();
            }
            self.pending_line.drain(..=newline);
            self.push_line(&line, on_chunk)?;
        }
        Ok(())
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
        for chunk in parse_chat_stream_line(line)? {
            match &chunk {
                ProviderStreamChunk::Reasoning(value) => self.thinking.push_str(value),
                ProviderStreamChunk::Text(value) => self.text.push_str(value),
            }
            on_chunk(chunk);
        }
        Ok(())
    }
}
