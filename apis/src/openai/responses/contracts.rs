// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 Praxis Contributors

//! Runtime JSON contracts for Praxis-transformed Responses operations.

#![expect(
    clippy::allow_attributes,
    clippy::large_stack_frames,
    reason = "utoipa macro-generated schema builders allocate large temporary values"
)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

/// Request body accepted by `POST /responses`.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[allow(dead_code, reason = "schema-only contract types for OpenAPI description generation")]
pub(super) struct CreateResponseRequest {
    /// Target model identifier.
    #[schema(example = "gpt-4o")]
    pub(super) model: Option<String>,

    /// Response input items or string prompt.
    pub(super) input: Option<Value>,

    /// System instructions.
    pub(super) instructions: Option<String>,

    /// Tools available for model execution.
    pub(super) tools: Option<Vec<Value>>,

    /// Tool selection policy.
    pub(super) tool_choice: Option<Value>,

    /// Sampling temperature.
    pub(super) temperature: Option<f64>,

    /// Nucleus sampling `top_p`.
    pub(super) top_p: Option<f64>,

    /// Whether to stream SSE response events.
    pub(super) stream: Option<bool>,

    /// Whether to store the response resource.
    pub(super) store: Option<bool>,
}

/// Response body emitted by `POST /responses`.
#[derive(Debug, Serialize, ToSchema)]
#[allow(dead_code, reason = "schema-only contract types for OpenAPI description generation")]
pub(super) struct ResponseResource {
    /// Unique response identifier.
    #[schema(example = "resp_123")]
    pub(super) id: String,

    /// Object type label. Always `response`.
    #[schema(example = "response")]
    pub(super) object: String,

    /// Generation status.
    #[schema(example = "completed")]
    pub(super) status: String,

    /// Creation timestamp in epoch seconds.
    pub(super) created_at: u64,

    /// Completion timestamp in epoch seconds.
    pub(super) completed_at: Option<u64>,

    /// Model used for response generation.
    pub(super) model: String,

    /// Generated output items.
    pub(super) output: Vec<Value>,

    /// Token usage details.
    pub(super) usage: Option<Value>,

    /// Error details when status is `failed`.
    pub(super) error: Option<Value>,

    /// Metadata map.
    pub(super) metadata: Option<Value>,

    /// Original response input items.
    pub(super) input: Option<Value>,

    /// System instructions.
    pub(super) instructions: Option<Value>,

    /// Tools available for execution.
    pub(super) tools: Option<Value>,

    /// Tool selection policy.
    pub(super) tool_choice: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_types_can_be_instantiated() {
        let req = CreateResponseRequest {
            model: Some("gpt-4o".to_owned()),
            input: None,
            instructions: None,
            tools: None,
            tool_choice: None,
            temperature: None,
            top_p: None,
            stream: None,
            store: None,
        };
        assert_eq!(req.model.as_deref(), Some("gpt-4o"));

        let res = ResponseResource {
            id: "resp_1".to_owned(),
            object: "response".to_owned(),
            status: "completed".to_owned(),
            created_at: 0,
            completed_at: None,
            model: "gpt-4o".to_owned(),
            output: Vec::new(),
            usage: None,
            error: None,
            metadata: None,
            input: None,
            instructions: None,
            tools: None,
            tool_choice: None,
        };
        assert_eq!(res.id, "resp_1");
    }
}
