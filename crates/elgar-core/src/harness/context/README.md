# Harness Context

This folder owns the currently executable read-only evidence gathering for
primitive harness tools.

Context files collect facts from the local project but do not decide when to use
those facts and do not mutate the filesystem.

Current files:

- `project_file.rs` backs primitive `read` with bounded single-file content.
- `directory.rs` backs primitive `ls` with bounded one-directory summaries.
- `find.rs` backs primitive `find` with bounded path matches.
- `grep.rs` backs primitive `grep` with bounded text matches.

Future files:

- `recent_turns.rs` for selected recent conversation evidence.
- `evidence_bundle.rs` for combining verified evidence before provider calls.
