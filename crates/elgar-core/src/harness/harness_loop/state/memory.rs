//! Short-term memory for one primitive harness turn.
//!
//! This memory is owned by Elgar, not the model. It tracks what the current
//! loop already inspected so duplicate/no-op requests can be logged and fed
//! back to the next model decision without consuming useful evidence budget.

use std::collections::{BTreeMap, BTreeSet};

use crate::harness::harness_loop::state::{
    listing_memory::DirectoryListingMemory, types::EvidenceKey,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::harness::harness_loop) struct HarnessWorkingMemory {
    listed_paths: BTreeSet<String>,
    directory_listings: BTreeMap<String, DirectoryListingMemory>,
    read_paths: BTreeSet<String>,
    find_patterns: BTreeSet<String>,
    grep_queries: BTreeSet<String>,
    mcp_calls: BTreeSet<String>,
    side_effects: Vec<String>,
    duplicate_requests: Vec<String>,
    duplicate_rejection_streak: usize,
}

impl HarnessWorkingMemory {
    /// Record one useful verified primitive request.
    pub fn record_useful_request(&mut self, key: &EvidenceKey) {
        self.duplicate_rejection_streak = 0;
        match key {
            EvidenceKey::Ls(path) => {
                self.listed_paths.insert(path.clone());
            }
            EvidenceKey::Read(path) => {
                self.read_paths.insert(path.clone());
            }
            EvidenceKey::Find(path, pattern) => {
                self.find_patterns.insert(format!("{path}:{pattern}"));
            }
            EvidenceKey::Grep(path, query) => {
                self.grep_queries.insert(format!("{path}:{query}"));
            }
            EvidenceKey::Mcp(server, tool, fingerprint) => {
                self.mcp_calls
                    .insert(format!("{server}:{tool}:{fingerprint}"));
            }
            EvidenceKey::InvalidMcp(fingerprint) => {
                self.mcp_calls.insert(format!("invalid:{fingerprint}"));
            }
            EvidenceKey::SideEffectVersion(tool, target, fingerprint) => {
                self.side_effects
                    .push(format!("side_effect:{tool}:{target}:{fingerprint}"));
            }
            EvidenceKey::SideEffectEpoch(tool, target, epoch) => {
                self.side_effects
                    .push(format!("side_effect:{tool}:{target}:epoch:{epoch}"));
            }
        }
    }

    /// Record one exact duplicate request from the current loop.
    pub fn record_duplicate_request(&mut self, label: impl Into<String>) {
        self.duplicate_requests.push(label.into());
        self.duplicate_rejection_streak += 1;
    }

    /// Record compact visible entries from a verified directory listing.
    pub fn record_directory_listing(&mut self, listing: DirectoryListingMemory) {
        if !listing.is_empty() {
            self.directory_listings
                .insert(listing.path.clone(), listing);
        }
    }

    pub fn duplicate_streak(&self) -> usize {
        self.duplicate_rejection_streak
    }

    pub fn listed_paths(&self) -> Vec<&str> {
        self.listed_paths.iter().map(String::as_str).collect()
    }

    pub fn directory_listings(&self) -> Vec<&DirectoryListingMemory> {
        self.directory_listings.values().collect()
    }

    pub fn read_paths(&self) -> Vec<&str> {
        self.read_paths.iter().map(String::as_str).collect()
    }

    pub fn find_patterns(&self) -> Vec<&str> {
        self.find_patterns.iter().map(String::as_str).collect()
    }

    pub fn grep_queries(&self) -> Vec<&str> {
        self.grep_queries.iter().map(String::as_str).collect()
    }

    pub fn duplicate_requests(&self) -> Vec<&str> {
        self.duplicate_requests.iter().map(String::as_str).collect()
    }

    pub fn side_effects(&self) -> Vec<&str> {
        self.side_effects.iter().map(String::as_str).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.listed_paths.is_empty()
            && self.read_paths.is_empty()
            && self.find_patterns.is_empty()
            && self.grep_queries.is_empty()
            && self.side_effects.is_empty()
            && self.duplicate_requests.is_empty()
    }
}

/// Render compact working memory for the next model decision prompt.
pub(in crate::harness::harness_loop) fn render_working_memory_for_prompt(
    memory: &HarnessWorkingMemory,
) -> String {
    if memory.is_empty() {
        return "(none)".to_string();
    }

    let mut lines = Vec::new();
    push_limited_group(&mut lines, "already listed", memory.listed_paths());
    push_listing_memory(&mut lines, memory.directory_listings());
    push_limited_group(&mut lines, "already read", memory.read_paths());
    push_limited_group(&mut lines, "already searched files", memory.find_patterns());
    push_limited_group(&mut lines, "already grepped", memory.grep_queries());
    push_limited_group(&mut lines, "verified side effects", memory.side_effects());
    push_limited_group(
        &mut lines,
        "duplicate/no-op requests",
        memory.duplicate_requests(),
    );
    for duplicate in memory.duplicate_requests() {
        lines.push(format!(
            "- Runtime rejected duplicate request `{duplicate}`; do not request it again this turn."
        ));
        if let Some(path) = duplicate.strip_prefix("ls:") {
            if let Some(listing) = memory.directory_listings.get(path) {
                lines.push(listing.render_duplicate_hint());
            }
        }
    }
    lines.push(
        "Exact duplicate requests are already known; choose different primitive evidence or return final text if existing evidence is enough."
            .to_string(),
    );
    lines.join("\n")
}

fn push_listing_memory(lines: &mut Vec<String>, listings: Vec<&DirectoryListingMemory>) {
    for listing in listings.into_iter().take(4) {
        lines.push(listing.render_for_prompt());
    }
}

fn push_limited_group(lines: &mut Vec<String>, label: &str, values: Vec<&str>) {
    if values.is_empty() {
        return;
    }

    let shown = values
        .iter()
        .take(6)
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let omitted = values.len().saturating_sub(6);
    if omitted == 0 {
        lines.push(format!("- {label}: {shown}"));
    } else {
        lines.push(format!("- {label}: {shown} (+{omitted} more)"));
    }
}
