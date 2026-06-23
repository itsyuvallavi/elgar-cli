# Context

This folder owns optional local context that can be attached to a model request.

This is not the TUI screen and not model reasoning. It is related to the LLM
context window: how much extra text Elgar might send along with the user input.

Files:

- `mod.rs` re-exports the public context API.
- `bundle.rs` builds prompt/context text from selected files.
- `accounting.rs` records what context was loaded or omitted.
- `loading.rs` finds local files and memory notes.
- `budget.rs` owns rough token estimates, context budget, and trimming.

The active harness currently sends only verified primitive evidence. Future
harness stages may use this folder when we add broader context-aware turns.

Overlap note:

- `context/budget.rs` estimates prompt size before sending a request.
- `token_accounting.rs` tracks provider-reported usage after a request.

Those are adjacent concepts, not duplicate behavior.
