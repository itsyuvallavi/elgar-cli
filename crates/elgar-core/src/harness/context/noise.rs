//! Shared noise-directory checks for read-only harness collectors.

const NOISY_DIRECTORIES: [&str; 7] = [
    ".git",
    ".elgar",
    ".next",
    "target",
    "node_modules",
    "dist",
    "build",
];

pub(super) fn is_noisy_directory(name: &str) -> bool {
    NOISY_DIRECTORIES.contains(&name)
}
