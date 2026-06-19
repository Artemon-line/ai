// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! AI guardrails filter: calls external content safety providers
//! (e.g. NeMo Guardrails) to evaluate request and response bodies.

mod config;
mod filter;
pub(crate) mod providers;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "tests"
)]
mod tests;

pub use filter::AiGuardrailsFilter;
