// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! `NeMo` Guardrails provider: calls `/v1/guardrail/checks` and maps
//! the response to [`GuardResult`].
//!
//! Full implementation in #578.

use async_trait::async_trait;
use serde::Deserialize;

use super::{GuardPhase, GuardProvider, GuardResult};
use crate::FilterError;

/// Default timeout for `NeMo` HTTP calls (10 seconds).
const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// `NeMo`-specific configuration fields.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NemoConfig {
    /// `NeMo` endpoint URL.
    endpoint: String,

    /// Per-request timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    timeout_ms: u64,
}

/// Returns the default timeout value for serde deserialization.
fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// `NeMo` Guardrails provider.
pub(in crate::builtins::http::ai::guardrails) struct NemoProvider {
    /// `NeMo` endpoint URL (e.g. `http://nemo:8000/v1/guardrail/checks`).
    #[expect(dead_code, reason = "used once HTTP calls are wired (#578)")]
    endpoint: String,

    /// Per-request timeout in milliseconds.
    #[expect(dead_code, reason = "used once HTTP calls are wired (#578)")]
    timeout_ms: u64,
}

impl NemoProvider {
    /// Parse and validate `NeMo`-specific config from the provider settings.
    pub fn from_config(config: &serde_yaml::Value) -> Result<Self, FilterError> {
        let cfg: NemoConfig = serde_yaml::from_value(config.clone())
            .map_err(|e| FilterError::from(format!("ai_guardrails (nemo): {e}")))?;

        if cfg.endpoint.is_empty() {
            return Err("ai_guardrails (nemo): 'endpoint' must not be empty".into());
        }
        if cfg.timeout_ms == 0 {
            return Err("ai_guardrails (nemo): 'timeout_ms' must be greater than zero".into());
        }

        Ok(Self {
            endpoint: cfg.endpoint,
            timeout_ms: cfg.timeout_ms,
        })
    }
}

#[async_trait]
impl GuardProvider for NemoProvider {
    async fn evaluate(
        &self,
        _messages: Vec<serde_json::Value>,
        _phase: GuardPhase,
    ) -> Result<GuardResult, FilterError> {
        // HTTP call to NeMo wired in #578.
        Ok(GuardResult::Pass)
    }
}
