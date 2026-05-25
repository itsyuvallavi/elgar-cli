# elgar-core

## Purpose

Core library for Elgar v0.2. It owns agent runtime flow, the explicit action gate, routes, sessions, actions, provider integration, filesystem boundaries, legacy controller compatibility, and renderer output.

## Important Folders

- `src` contains core modules and in-module runtime/controller tests.
- `tests` contains crate-level regression coverage.

## Ownership

Runtime owns normal chat flow. Provider text suggests actions; core applies, verifies, and reports filesystem results through explicit typed paths.

## Checks

- `cargo test -p elgar-core`
- `cargo test -p elgar-core --test core_harness_regression`
