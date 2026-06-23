# code_blocks

## Purpose

Render fenced code blocks as compact terminal boxes.

## Files

- `mod.rs` owns the public code block rendering API.
- `fence.rs` parses fence metadata such as language and path labels.
- `box_render.rs` draws the terminal box around visible code lines.
- `wrap.rs` wraps long code lines with continuation markers.
- `tests.rs` verifies headers, inferred language labels, and wrapping.

## Notes

This folder only controls display formatting. Raw assistant text remains
available through the conversation details/copy path.
