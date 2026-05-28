use std::time::Duration;

use elgar_core::event::{Event, ProviderTokenUsage};

pub(crate) fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

pub(crate) fn aggregate_provider_token_usage(events: &[Event]) -> Option<ProviderTokenUsage> {
    let mut prompt_tokens = 0u64;
    let mut completion_tokens = 0u64;
    let mut total_tokens = 0u64;
    let mut saw_prompt = false;
    let mut saw_completion = false;
    let mut saw_total = false;

    for event in events {
        let Event::ProviderFinished(finished) = event else {
            continue;
        };
        let Some(usage) = finished
            .output
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.usage.as_ref())
        else {
            continue;
        };

        if let Some(tokens) = usage.prompt_tokens {
            prompt_tokens = prompt_tokens.saturating_add(tokens);
            saw_prompt = true;
        }
        if let Some(tokens) = usage.completion_tokens {
            completion_tokens = completion_tokens.saturating_add(tokens);
            saw_completion = true;
        }
        if let Some(tokens) = usage.total_tokens {
            total_tokens = total_tokens.saturating_add(tokens);
            saw_total = true;
        }
    }

    if !saw_prompt && !saw_completion && !saw_total {
        return None;
    }

    Some(ProviderTokenUsage {
        prompt_tokens: saw_prompt.then_some(prompt_tokens),
        completion_tokens: saw_completion.then_some(completion_tokens),
        total_tokens: if saw_total {
            Some(total_tokens)
        } else if saw_prompt || saw_completion {
            Some(prompt_tokens.saturating_add(completion_tokens))
        } else {
            None
        },
    })
}
