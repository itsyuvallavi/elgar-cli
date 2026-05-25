# model_first_policy

## Purpose

Regression tests for model-first policy behavior and user-visible safety guidance around legacy controller paths.

## Important Files

- `guidance_and_uncertainty.rs` covers uncertain or guidance-heavy requests.
- `safe_create_policy.rs` covers safe create-vs-modify policy decisions.
- `visible_text.rs` covers what provider text is allowed to show.

## Ownership

Keep these tests deterministic and policy-focused. Do not depend on live model output.

## Checks

- `cargo test -p elgar-core model_first_policy`
- `cargo test -p elgar-core model_first`
