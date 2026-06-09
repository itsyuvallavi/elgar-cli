# Harness Loop State

Shared loop support code.

## Files

- `mod.rs` exposes state modules.
- `budget.rs` tracks duplicate evidence and repair-attempt guards.
- `listing_memory.rs` stores capped visible dirs/files from verified `ls` results.
- `logging.rs` writes provider, model-choice, evidence, and finish events.
- `memory.rs` tracks short-term same-turn harness memory and duplicate counts.
- `types.rs` defines evidence and loop result types.

State code should not call providers or execute primitive tools.

Exact duplicate primitive requests are treated as no-op work inside one harness
turn. The first duplicate is shown back to the model as memory; the second
duplicate stops the loop with `duplicate_loop_detected` so synthesis can answer
from verified evidence.

For verified `ls` results, memory keeps a compact list of visible child
directories and files. This helps the model choose a more specific next
primitive instead of repeating the same directory listing. The listing memory is
capped and same-turn only.

Useful read-only evidence is not capped here by item count, byte count, or
primitive type. Duplicate evidence is still tracked because repeating the same
primitive request does not add new verified information.
