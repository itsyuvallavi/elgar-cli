# terminal/input

## Purpose

This folder owns text input before it becomes a model request or a local command.

## Files

- `mod.rs` exposes the input modules to the parent terminal module.
- `keymap.rs` translates keyboard and paste events into input actions.
- `raw_mode.rs` enters and exits terminal raw mode safely.
- `normalization.rs` cleans pasted terminal transcripts before submission.
- `read.rs` reads prompt input until the user submits text, activates an
  approval button, or exits.

## Rule

Input code should not call the provider and should not render conversation output.

When approval is pending and the prompt is empty, `Tab` or horizontal arrows
switch the selected text button and `Enter` submits that approval action. When
the prompt contains text, normal text editing and submission behavior wins.

Terminal raw mode is not model routing:

- terminal raw mode lets Elgar read keys directly.
- model routing happens through the core harness.
