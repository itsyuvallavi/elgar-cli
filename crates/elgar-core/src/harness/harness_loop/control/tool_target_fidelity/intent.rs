//! User-language parsing for direct primitive requests.

use crate::harness::harness_loop::evidence::keys::normalize_evidence_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::harness::harness_loop::control) enum ExplicitPrimitiveRequest {
    Read { path: String },
    List { path: String },
    Grep { query: String, path: String },
}

impl ExplicitPrimitiveRequest {
    pub(in crate::harness::harness_loop::control) fn missing_evidence_feedback(&self) -> String {
        format!(
            "The user directly requested `{}`. This turn has no verified tool evidence. Request the matching primitive tool now. Do not answer from memory, prior chat, or claim the path exists or does not exist without verified tool evidence.",
            self.request_label()
        )
    }

    fn request_label(&self) -> String {
        match self {
            Self::Read { path } => format!("file evidence for {path}"),
            Self::List { path } => format!("directory evidence for {path}"),
            Self::Grep { query, path } => format!("search for {query} in {path}"),
        }
    }
}

pub(in crate::harness::harness_loop::control) fn explicit_primitive_request(
    input: &str,
) -> Option<ExplicitPrimitiveRequest> {
    if let Some(request) = explicit_read_request(input) {
        return Some(request);
    }
    if let Some(request) = explicit_list_request(input) {
        return Some(request);
    }
    explicit_grep_request(input)
}

fn explicit_read_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let trimmed = input.trim();
    let read_path = strip_any_prefix(trimmed, &["read ", "open ", "show me "])?;
    let path = first_pathish_token(read_path);
    if !path.contains(' ') && is_file_like_path(path) {
        return Some(ExplicitPrimitiveRequest::Read {
            path: normalize_evidence_path(path),
        });
    }

    None
}

fn explicit_list_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let trimmed = input.trim();
    let list_path = strip_any_prefix(trimmed, &["ls ", "list ", "show me "])?;
    let path = strip_folder_suffix(list_path)?;
    if path.contains(' ') || path.is_empty() {
        return None;
    }
    Some(ExplicitPrimitiveRequest::List {
        path: normalize_evidence_path(path),
    })
}

fn explicit_grep_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let trimmed = input.trim();
    if let Some(request) = explicit_search_inside_request(trimmed) {
        return Some(request);
    }
    let rest = strip_any_prefix(trimmed, &["grep ", "search for ", "find ", "look for "])?;
    let (query, path) = rest.split_once(" in ")?;
    if query.trim().contains(' ') || path.trim().contains(' ') {
        return None;
    }
    Some(ExplicitPrimitiveRequest::Grep {
        query: strip_wrapping_punctuation(query).trim().to_string(),
        path: normalize_evidence_path(strip_wrapping_punctuation(path)),
    })
}

fn explicit_search_inside_request(input: &str) -> Option<ExplicitPrimitiveRequest> {
    let rest = input.strip_prefix("search inside ")?;
    let (path, query) = rest.split_once(" for ")?;
    if query.trim().contains(' ') || path.trim().contains(' ') {
        return None;
    }
    Some(ExplicitPrimitiveRequest::Grep {
        query: strip_wrapping_punctuation(query).trim().to_string(),
        path: normalize_evidence_path(strip_wrapping_punctuation(path)),
    })
}

fn strip_any_prefix<'a>(value: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes
        .iter()
        .find_map(|prefix| value.strip_prefix(prefix))
}

fn strip_wrapping_punctuation(value: &str) -> &str {
    value
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim_matches('\'')
}

fn first_pathish_token(value: &str) -> &str {
    let token = value.split_whitespace().next().unwrap_or(value);
    strip_sentence_punctuation(strip_wrapping_punctuation(token))
}

fn strip_sentence_punctuation(value: &str) -> &str {
    value
        .trim_end_matches('.')
        .trim_end_matches(',')
        .trim_end_matches(':')
        .trim_end_matches(';')
}

fn strip_folder_suffix(value: &str) -> Option<&str> {
    let value = strip_wrapping_punctuation(value.trim());
    let value = value.strip_prefix("the ").unwrap_or(value);
    for suffix in [" folder", " directory", "/"] {
        if let Some(path) = value.strip_suffix(suffix) {
            return Some(strip_wrapping_punctuation(path.trim()));
        }
    }
    None
}

fn is_file_like_path(path: &str) -> bool {
    path.contains('/')
        || path
            .rsplit('/')
            .next()
            .is_some_and(|name| name.contains('.'))
}
