pub(crate) fn format_provider_reasoning_summary(text: &str, max_chars: usize) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "need" | "need to" | "we" | "we need" | "we need to"
    ) {
        return None;
    }

    let formatted = if let Some(rest) = strip_prefix_case_insensitive(text, "we need to ") {
        progress_note_from_need(rest)
    } else if let Some(rest) = strip_prefix_case_insensitive(text, "need to ") {
        progress_note_from_need(rest)
    } else if let Some(rest) = strip_prefix_case_insensitive(text, "need ") {
        progress_note_from_need(rest)
    } else if let Some(rest) = strip_prefix_case_insensitive(text, "we just ") {
        progress_note_from_action(rest)
    } else if let Some(rest) = strip_prefix_case_insensitive(text, "just ") {
        progress_note_from_action(rest)
    } else {
        normalize_reasoning_instruction(text)
    };

    let formatted = truncate_chars(&formatted, max_chars);
    if formatted.is_empty() || is_low_value_reasoning_summary(&formatted) {
        None
    } else {
        Some(formatted)
    }
}

fn strip_prefix_case_insensitive<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        .then(|| text[prefix.len()..].trim())
}

fn progress_note_from_need(text: &str) -> String {
    let text = normalize_reasoning_instruction(text);
    if text.is_empty() {
        return text;
    }

    let mut words = text.splitn(2, ' ');
    let first = words.next().unwrap_or_default();
    let rest = words.next().unwrap_or_default();
    let first = first
        .trim_end_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase();
    let verb = match first.as_str() {
        "greet" | "greeting" => "Greeting",
        "answer" => "Answering",
        "respond" => "Responding",
        "reply" => "Replying",
        "explain" => "Explaining",
        "summarize" => "Summarizing",
        "check" => "Checking",
        "inspect" => "Inspecting",
        "review" => "Reviewing",
        "read" => "Reading",
        "test" => "Testing",
        "verify" => "Verifying",
        "use" => "Using",
        _ => return text,
    };

    format_progress_note(verb, rest)
}

fn progress_note_from_action(text: &str) -> String {
    let text = normalize_reasoning_instruction(text);
    if text.is_empty() {
        return text;
    }

    let mut words = text.splitn(2, ' ');
    let first = words.next().unwrap_or_default();
    let rest = words.next().unwrap_or_default();
    let first = first
        .trim_end_matches(|character: char| !character.is_alphanumeric())
        .to_ascii_lowercase();

    let verb = match first.as_str() {
        "greet" | "greeting" => "Greeting",
        "answer" => "Answering",
        "respond" => "Responding",
        "reply" => "Replying",
        "explain" => "Explaining",
        "summarize" => "Summarizing",
        "check" => "Checking",
        "inspect" => "Inspecting",
        "review" => "Reviewing",
        "read" => "Reading",
        "test" => "Testing",
        "verify" => "Verifying",
        "use" => "Using",
        _ => return text,
    };

    format_progress_note(verb, rest)
}

fn format_progress_note(verb: &str, rest: &str) -> String {
    let rest = remove_reasoning_instruction_filler(rest);
    let rest = trim_leading_reasoning_punctuation(&rest);
    if rest.is_empty()
        || matches!(
            rest.to_ascii_lowercase().as_str(),
            "greet" | "greet." | "greeting" | "greeting."
        )
    {
        return format!("{verb}.");
    }

    if rest.chars().next().is_some_and(char::is_uppercase) {
        format!("{verb}. {rest}")
    } else {
        format!("{verb} {rest}")
    }
}

fn normalize_reasoning_instruction(text: &str) -> String {
    let text = remove_reasoning_instruction_filler(text);
    normalize_sentence(&text)
}

fn remove_reasoning_instruction_filler(text: &str) -> String {
    let mut text = text.trim().to_string();
    if text.is_empty() {
        return text;
    }

    for phrase in [
        "as Elgar",
        "terminal-friendly prose",
        "terminal friendly prose",
        "terminal-friendly style",
        "terminal friendly style",
    ] {
        text = replace_case_insensitive(&text, phrase, "");
    }
    for word in [
        "briefly",
        "brief",
        "shortly",
        "short",
        "concisely",
        "concise",
        "succinctly",
        "succinct",
    ] {
        text = remove_word_case_insensitive(&text, word);
    }

    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = cleanup_punctuation_spacing(&text);
    let text = text
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | ':' | '-')
        })
        .to_string();

    if matches!(
        text.to_ascii_lowercase().as_str(),
        "in" | "in prose" | "prose"
    ) {
        String::new()
    } else {
        text
    }
}

fn cleanup_punctuation_spacing(text: &str) -> String {
    let mut cleaned = text.to_string();
    for punctuation in [",", ".", ";", ":"] {
        cleaned = cleaned.replace(&format!(" {punctuation}"), punctuation);
    }
    cleaned
}

fn trim_leading_reasoning_punctuation(text: &str) -> String {
    text.trim_start_matches(|character: char| {
        character.is_whitespace() || matches!(character, ',' | ';' | ':' | '-' | '.')
    })
    .to_string()
}

fn replace_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    let mut remaining = text;
    let mut output = String::new();
    while let Some(index) = remaining
        .to_ascii_lowercase()
        .find(&needle.to_ascii_lowercase())
    {
        output.push_str(&remaining[..index]);
        output.push_str(replacement);
        remaining = &remaining[index + needle.len()..];
    }
    output.push_str(remaining);
    output
}

fn remove_word_case_insensitive(text: &str, word: &str) -> String {
    text.split_whitespace()
        .filter(|candidate| {
            let normalized = candidate
                .trim_matches(|character: char| !character.is_alphanumeric())
                .to_ascii_lowercase();
            normalized != word
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_sentence(text: &str) -> String {
    let mut text = text.trim().to_string();
    if text.is_empty() {
        return text;
    }

    if let Some(first) = text.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    text
}

fn is_low_value_reasoning_summary(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "answering." | "responding." | "replying."
    )
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}
