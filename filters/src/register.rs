// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Public AI filter registration for consumers outside `praxis-ai-proxy`.

use std::sync::Arc;

use praxis_core::subrequest::SubRequestClient;
use praxis_filter::{FilterError, FilterFactory, FilterRegistry, HttpFilter};

use crate::{
    A2aFilter, AiGuardrailsFilter, IntelligentRouteFilter, McpFilter, ModelToHeaderFilter, PromptEnrichFilter,
    ReloadableSubRequestClient, TimeToFirstTokenFilter, TokenCountFilter, TokenUsageHeadersFilter,
};

/// Register all in-tree AI HTTP filters into `registry`.
///
/// When `subrequest_client` is provided, the filters that make HTTP callouts
/// capture a clone of the shared [`ReloadableSubRequestClient`] handle instead
/// of creating isolated per-filter connectors. Each such factory reads the
/// current client from the handle at *build* time, so filters rebuilt during a
/// hot config reload observe the new `body_limits.max_response_bytes` ceiling.
/// The client-aware filters are:
///
/// - `anthropic_web_search`
/// - `openai_file_resolve`
/// - `openai_responses_compact`
/// - `openai_file_search_callout`
/// - `openai_web_search`
///
/// Does not call [`FilterRegistry::with_builtins`].
/// Does not register auto-discovered external filters.
///
/// Pipelines that use OpenAI store or rehydrate filters must also install:
///
/// ```rust,ignore
/// pipeline.add_pipeline_extension(
///     Box::new(praxis_ai_apis::store::ResponseStoreRegistry::new()),
/// );
/// ```
pub fn register_ai_filters(registry: &mut FilterRegistry, subrequest_client: Option<&ReloadableSubRequestClient>) {
    register_agentic_filters(registry);
    register_general_ai_filters(registry);
    register_anthropic_filters(registry, subrequest_client);
    register_openai_filters(registry, subrequest_client);
    register_routing_filters(registry);
}

/// Build a [`FilterRegistry`] with core builtins and in-tree AI filters.
///
/// Equivalent to [`FilterRegistry::with_builtins`] followed by
/// [`register_ai_filters`] with no shared sub-request client. Does
/// not register auto-discovered external filters.
///
/// Filters that make HTTP callouts create isolated per-filter
/// connectors. Use [`register_ai_filters`] with a shared client
/// when the server runtime is available.
///
/// Pipelines that use OpenAI store or rehydrate filters must also install
/// [`praxis_ai_apis::store::ResponseStoreRegistry`] as a pipeline extension.
#[must_use]
pub fn build_ai_registry() -> FilterRegistry {
    let mut registry = FilterRegistry::with_builtins();
    register_ai_filters(&mut registry, None);
    registry
}

/// Register agentic protocol filters (A2A, MCP).
fn register_agentic_filters(registry: &mut FilterRegistry) {
    praxis_filter::register_filters!(
        @register registry,
        http "a2a" => A2aFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "mcp" => McpFilter::from_config
    );
}

/// Register general-purpose AI filters.
fn register_general_ai_filters(registry: &mut FilterRegistry) {
    praxis_filter::register_filters!(
        @register registry,
        http "ai_guardrails" => AiGuardrailsFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "model_to_header" => ModelToHeaderFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "prompt_enrich" => PromptEnrichFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "token_count" => TokenCountFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "token_usage_headers" => TokenUsageHeadersFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "time_to_first_token" => TimeToFirstTokenFilter::from_config
    );
}

/// Register intelligent routing filters.
fn register_routing_filters(registry: &mut FilterRegistry) {
    praxis_filter::register_filters!(
        @register registry,
        http "intelligent_route" => IntelligentRouteFilter::from_config
    );
}

/// Register Anthropic-specific filters.
fn register_anthropic_filters(registry: &mut FilterRegistry, subrequest_client: Option<&ReloadableSubRequestClient>) {
    praxis_filter::register_filters!(
        @register registry,
        http "anthropic_messages_format" => praxis_ai_apis::anthropic::AnthropicMessagesFormatFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "anthropic_messages_protocol" => praxis_ai_apis::anthropic::AnthropicMessagesProtocolFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "anthropic_stream_events" => praxis_ai_apis::anthropic::AnthropicStreamEventsFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "anthropic_to_openai" => praxis_ai_apis::anthropic::AnthropicToOpenaiFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "anthropic_validate" => praxis_ai_apis::anthropic::AnthropicValidateFilter::from_config
    );
    register_client_aware(
        registry,
        "anthropic_web_search",
        subrequest_client,
        praxis_ai_apis::anthropic::AnthropicWebSearchFilter::from_config_with_client,
        praxis_ai_apis::anthropic::AnthropicWebSearchFilter::from_config,
    );
}

/// Register OpenAI Responses API request-path filters.
fn register_openai_filters(registry: &mut FilterRegistry, subrequest_client: Option<&ReloadableSubRequestClient>) {
    register_openai_responses_filters(registry, subrequest_client);
    praxis_filter::register_filters!(
        @register registry,
        http "openai_conversations" => praxis_ai_apis::openai::OpenaiConversationsFilter::from_config
    );
}

/// Register OpenAI Responses API filters.
fn register_openai_responses_filters(
    registry: &mut FilterRegistry,
    subrequest_client: Option<&ReloadableSubRequestClient>,
) {
    register_client_aware(
        registry,
        "openai_file_resolve",
        subrequest_client,
        praxis_ai_apis::openai::FileResolveFilter::from_config_with_client,
        praxis_ai_apis::openai::FileResolveFilter::from_config,
    );
    register_client_aware(
        registry,
        "openai_responses_compact",
        subrequest_client,
        praxis_ai_apis::openai::CompactFilter::from_config_with_client,
        praxis_ai_apis::openai::CompactFilter::from_config,
    );
    register_client_aware(
        registry,
        "openai_file_search_callout",
        subrequest_client,
        praxis_ai_apis::openai::FileSearchCalloutFilter::from_config_with_client,
        praxis_ai_apis::openai::FileSearchCalloutFilter::from_config,
    );
    register_openai_responses_transform_filters(registry);
    register_openai_response_filters(registry, subrequest_client);
}

/// Register OpenAI Responses request-path filters that need no shared client.
fn register_openai_responses_transform_filters(registry: &mut FilterRegistry) {
    praxis_filter::register_filters!(
        @register registry,
        http "openai_doc_extract" => praxis_ai_apis::openai::DocExtractFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_responses_format" => praxis_ai_apis::openai::ResponsesFormatFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_responses_model_rewrite" => praxis_ai_apis::openai::ModelRewriteFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_responses_validate" => praxis_ai_apis::openai::OpenaiResponsesValidateFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_responses_rehydrate" => praxis_ai_apis::openai::RehydrateFilter::from_config
    );
}

/// Register OpenAI Responses API response-path and persistence filters.
fn register_openai_response_filters(
    registry: &mut FilterRegistry,
    subrequest_client: Option<&ReloadableSubRequestClient>,
) {
    praxis_filter::register_filters!(
        @register registry,
        http "openai_response_store" => praxis_ai_apis::openai::ResponseStoreFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_stream_events" => praxis_ai_apis::openai::OpenaiStreamEventsFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_responses_proxy" => praxis_ai_apis::openai::ResponsesProxyFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "responses_to_chat_completions" => praxis_ai_apis::openai::ResponsesToChatCompletionsFilter::from_config
    );
    register_client_aware(
        registry,
        "openai_web_search",
        subrequest_client,
        praxis_ai_apis::openai::WebSearchFilter::from_config_with_client,
        praxis_ai_apis::openai::WebSearchFilter::from_config,
    );
    register_openai_agentic_filters(registry);
}

/// Register OpenAI agentic loop, MCP dispatch, and tool-resolution filters.
fn register_openai_agentic_filters(registry: &mut FilterRegistry) {
    praxis_filter::register_filters!(
        @register registry,
        http "openai_mcp_tool_resolve" => praxis_ai_apis::openai::McpToolResolveFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_tool_parse" => praxis_ai_apis::openai::ToolParseFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "openai_mcp_dispatch" => praxis_ai_apis::openai::McpDispatchFilter::from_config
    );
    praxis_filter::register_filters!(
        @register registry,
        http "agentic_loop" => praxis_ai_apis::openai::AgenticLoopFilter::from_config
    );
}

// -----------------------------------------------------------------------------
// Sub-request-aware registration
// -----------------------------------------------------------------------------

/// Register a client-aware filter factory under `name`.
///
/// When `subrequest_client` is `Some`, the registered factory captures a clone
/// of the shared [`ReloadableSubRequestClient`] handle and reads the current
/// client via [`current`](ReloadableSubRequestClient::current) on *every*
/// build. This is the crux of the reload fix: a filter rebuilt during a hot
/// config reload observes the reloaded `body_limits.max_response_bytes` ceiling
/// rather than a client captured once at startup. When `None`, `fallback`
/// builds the filter with an isolated per-filter connector.
///
/// All client-aware AI filters register through this one seam so a factory
/// cannot accidentally pin itself to the startup client; the unit tests below
/// exercise the shared-handle path generically.
#[expect(clippy::panic, reason = "matches register_filters! macro convention")]
fn register_client_aware<C, F>(
    registry: &mut FilterRegistry,
    name: &'static str,
    subrequest_client: Option<&ReloadableSubRequestClient>,
    with_client: C,
    fallback: F,
) where
    C: Fn(&serde_yaml::Value, SubRequestClient) -> Result<Box<dyn HttpFilter>, FilterError> + Send + Sync + 'static,
    F: Fn(&serde_yaml::Value) -> Result<Box<dyn HttpFilter>, FilterError> + Send + Sync + 'static,
{
    let factory = match subrequest_client {
        Some(client) => {
            let client = client.clone();
            FilterFactory::Http(Arc::new(move |config| with_client(config, client.current())))
        },
        None => FilterFactory::Http(Arc::new(fallback)),
    };
    registry
        .register(name, factory)
        .unwrap_or_else(|_| panic!("duplicate filter name: '{name}'"));
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines, reason = "tests")]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use praxis_core::subrequest::{SubRequestClient, SubRequestConnector};
    use praxis_filter::{FilterAction, FilterError, FilterRegistry, HttpFilter, HttpFilterContext};

    use super::{ReloadableSubRequestClient, build_ai_registry, register_client_aware};

    /// Minimal filter returned by the test factories; it is never executed.
    struct ProbeFilter;

    #[async_trait]
    impl HttpFilter for ProbeFilter {
        fn name(&self) -> &'static str {
            "client_aware_probe"
        }

        async fn on_request(&self, _ctx: &mut HttpFilterContext<'_>) -> Result<FilterAction, FilterError> {
            Ok(FilterAction::Continue)
        }
    }

    /// Stable identity of the connector backing `client`, preserved across the
    /// clones `current()` returns but distinct for a client built with a fresh
    /// connector. Used to detect whether a factory re-reads the shared handle.
    fn connector_id(client: &SubRequestClient) -> usize {
        std::ptr::from_ref(client.connector().connector()) as usize
    }

    fn probe_client(pool_size: usize) -> SubRequestClient {
        SubRequestClient::new(SubRequestConnector::new(pool_size, None))
    }

    #[test]
    fn client_aware_factory_reads_current_client_at_each_build() {
        let recorded: Arc<Mutex<Vec<usize>>> = Arc::new(Mutex::new(Vec::new()));
        let probe = {
            let recorded = Arc::clone(&recorded);
            move |_config: &serde_yaml::Value, client: SubRequestClient| -> Result<Box<dyn HttpFilter>, FilterError> {
                recorded.lock().unwrap().push(connector_id(&client));
                Ok(Box::new(ProbeFilter))
            }
        };

        let handle = ReloadableSubRequestClient::new(probe_client(8));
        let startup_id = connector_id(&handle.current());

        let mut registry = FilterRegistry::with_builtins();
        register_client_aware(&mut registry, "client_aware_probe", Some(&handle), probe, |_| {
            panic!("fallback must not run when a shared client is provided")
        });

        let config = serde_yaml::Value::Null;
        registry.create("client_aware_probe", &config).unwrap();

        // Swap in a client backed by a fresh connector, as a hot reload does.
        let reloaded = probe_client(16);
        let reloaded_id = connector_id(&reloaded);
        handle.store(Arc::new(reloaded));
        registry.create("client_aware_probe", &config).unwrap();

        let ids = recorded.lock().unwrap().clone();
        assert_eq!(ids.len(), 2, "factory should build once per create call");
        assert_eq!(
            ids.first().copied(),
            Some(startup_id),
            "first build must observe the startup client"
        );
        assert_eq!(
            ids.get(1).copied(),
            Some(reloaded_id),
            "second build must observe the reloaded client; a factory that retained \
             the startup client would report the startup id here"
        );
    }

    #[test]
    fn client_aware_factory_uses_fallback_without_shared_client() {
        let fallback_builds = Arc::new(Mutex::new(0_usize));
        let fallback = {
            let fallback_builds = Arc::clone(&fallback_builds);
            move |_config: &serde_yaml::Value| -> Result<Box<dyn HttpFilter>, FilterError> {
                *fallback_builds.lock().unwrap() += 1;
                Ok(Box::new(ProbeFilter))
            }
        };

        let mut registry = FilterRegistry::with_builtins();
        register_client_aware(
            &mut registry,
            "client_aware_probe_fallback",
            None,
            |_config, _client| panic!("shared-client path must not run without a shared client"),
            fallback,
        );

        registry
            .create("client_aware_probe_fallback", &serde_yaml::Value::Null)
            .unwrap();

        assert_eq!(
            *fallback_builds.lock().unwrap(),
            1,
            "the fallback builder should run when no shared client is provided"
        );
    }

    #[test]
    fn build_ai_registry_includes_ai_and_builtin_filters() {
        let registry = build_ai_registry();
        let names = registry.available_filters();
        assert!(names.contains(&"ai_guardrails"), "expected ai_guardrails in registry");
        assert!(
            names.contains(&"openai_responses_validate"),
            "expected openai_responses_validate in registry"
        );
        assert!(
            names.contains(&"responses_to_chat_completions"),
            "expected responses_to_chat_completions in registry"
        );
        assert!(names.contains(&"a2a"), "expected agentic filter a2a in registry");
        assert!(
            names.contains(&"intelligent_route"),
            "expected intelligent_route in registry"
        );
        assert!(
            names.contains(&"anthropic_validate"),
            "expected anthropic filter in registry"
        );
        assert!(
            names.contains(&"anthropic_web_search"),
            "expected anthropic_web_search in registry"
        );
        assert!(
            names.contains(&"request_id"),
            "expected core builtin request_id in registry"
        );
    }
}
