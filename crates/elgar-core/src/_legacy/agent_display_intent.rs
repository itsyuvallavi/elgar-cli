pub(crate) fn should_stop_after_verified_display_only_shell(
    input: &str,
    is_project_listing: bool,
    is_direct_file_read: bool,
) -> bool {
    (input_requests_listing_display_only(input) && is_project_listing)
        || (input_requests_file_display_only(input) && is_direct_file_read)
}

fn input_requests_listing_display_only(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let asks_listing = lower.contains("project tree")
        || lower.contains("file tree")
        || lower.contains("folder tree")
        || lower.contains("show me the files")
        || lower.contains("show files")
        || lower.contains("list files")
        || lower.contains("list the files");
    asks_listing && !input_requests_analysis(&lower)
}

fn input_requests_file_display_only(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let asks_read = lower.starts_with("read ")
        || lower.starts_with("show ")
        || lower.starts_with("cat ")
        || lower.starts_with("open ");
    asks_read && lower.contains('.') && !input_requests_analysis(&lower)
}

pub(crate) fn input_requests_analysis(lower: &str) -> bool {
    [
        "review",
        "analyze",
        "analyse",
        "explain",
        "summarize",
        "summarise",
        "tell me",
        "describe",
        "what do you think",
        "what does",
        "what do",
        "render",
        "production",
        "findings",
        "key reason",
        "passed or failed",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}
