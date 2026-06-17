//! Public entry points for the primitive harness loop.
//!
//! This module keeps convenience wrappers out of the loop coordinator.

use crate::{
    event::Event,
    harness::{
        harness_loop::{
            control::coordinator::run_primitive_harness_loop_with_cancel_and_stream,
            state::types::PrimitiveHarnessLoopResult,
        },
        ModelChoiceTurnError,
    },
    provider::{ControllerProvider, ProviderCancelToken},
    session::Session,
};

/// Run a primitive loop for one harness turn.
pub fn run_primitive_harness_loop<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    run_primitive_harness_loop_with_cancel(provider, session, input, &ProviderCancelToken::new())
}

/// Run a primitive loop for one harness turn with cooperative cancellation.
pub fn run_primitive_harness_loop_with_cancel<P>(
    provider: &P,
    session: &mut Session,
    input: &str,
    cancel: &ProviderCancelToken,
) -> Result<PrimitiveHarnessLoopResult, ModelChoiceTurnError>
where
    P: ControllerProvider,
{
    let mut ignore_stream_event = |_event: Event| {};
    run_primitive_harness_loop_with_cancel_and_stream(
        provider,
        session,
        input,
        cancel,
        &mut ignore_stream_event,
    )
}
