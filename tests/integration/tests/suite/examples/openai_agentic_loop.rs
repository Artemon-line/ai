// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Integration tests for the openai_agentic_loop filter with
//! `iterative_request_router`.
//!
//! These tests verify that IRR, request-supplied MCP resolution,
//! MCP dispatch, and the agentic inference loop function together.

use std::collections::HashMap;

use praxis_test_utils::{
    McpMockConfig, McpToolFixture, StatefulCapturingBackend, build_pipeline, example_config_path, free_port, http_send,
    json_post, parse_body, parse_status, patch_yaml, start_mcp_mock_server_with_config, start_proxy,
    start_reloadable_proxy,
};

// -----------------------------------------------------------------------------
// Web search reload ceiling constants
// -----------------------------------------------------------------------------

/// Large sub-request response ceiling, 64 KiB. Comfortably above
/// [`WS_SEARCH_BODY_BYTES`], so the search callout resolves and the loop
/// completes.
const WS_LARGE_CEILING: usize = 64 * 1_024;

/// Small sub-request response ceiling, 1 KiB. Below [`WS_SEARCH_BODY_BYTES`], so
/// the search callout body exceeds the ceiling and is rejected.
const WS_SMALL_CEILING: usize = 1_024;

/// Size of the mock search provider's response body, 4 KiB. Chosen to sit
/// between [`WS_SMALL_CEILING`] and [`WS_LARGE_CEILING`] so the same callout is
/// admitted under the large ceiling and rejected under the small one.
const WS_SEARCH_BODY_BYTES: usize = 4 * 1_024;

// -----------------------------------------------------------------------------
// Pipeline Build
// -----------------------------------------------------------------------------

#[test]
fn example_config_builds_pipeline() {
    let config = load_agentic_config(free_port(), 19901);
    let _pipeline = build_pipeline(&config);
}

// -----------------------------------------------------------------------------
// Single-Pass
// -----------------------------------------------------------------------------

#[test]
fn single_pass_completes_through_irr() {
    let response = r#"{"id":"resp_1","object":"response","status":"completed","output":[]}"#;
    let model = StatefulCapturingBackend::new(vec![(200, response.to_owned())]).start_with_shutdown();
    let proxy_port = free_port();

    let config = load_agentic_config(proxy_port, model.port());
    let proxy = start_proxy(&config);

    let body = r#"{"model":"gpt-4.1","input":"Hello"}"#;
    let raw = http_send(proxy.addr(), &json_post("/v1/responses", body));

    assert_eq!(
        parse_status(&raw),
        200,
        "single-pass request through IRR should return 200"
    );

    let model_reqs = model.requests();
    assert_eq!(model_reqs.len(), 1, "model backend should receive one request");
    let model_body: serde_json::Value =
        serde_json::from_str(&model_reqs[0].body).expect("model request body should be valid JSON");
    assert_eq!(
        model_body["parallel_tool_calls"], false,
        "first inference must disable parallel tool calls when the client omits the field"
    );
}

#[test]
fn explicit_false_preserves_original_request_bytes() {
    let response = r#"{"id":"resp_1","object":"response","status":"completed","output":[]}"#;
    let model = StatefulCapturingBackend::new(vec![(200, response.to_owned())]).start_with_shutdown();
    let proxy_port = free_port();

    let config = load_agentic_config(proxy_port, model.port());
    let proxy = start_proxy(&config);

    let body = r#"{ "model": "gpt-4.1", "input": [{"role":"user","content":"Hello"}], "parallel_tool_calls": false }"#;
    let raw = http_send(proxy.addr(), &json_post("/v1/responses", body));

    assert_eq!(parse_status(&raw), 200);
    let model_reqs = model.requests();
    assert_eq!(model_reqs.len(), 1, "model backend should receive one request");
    assert_eq!(
        model_reqs[0].body, body,
        "an already-disabled request should retain byte-exact passthrough"
    );
}

#[test]
fn client_function_call_returns_without_server_execution() {
    let function_response = serde_json::json!({
        "id": "resp_client_tool",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "function_call",
            "id": "fc_client",
            "call_id": "call_client",
            "name": "get_weather",
            "arguments": r#"{"location":"SF"}"#,
            "status": "completed"
        }]
    });
    let model = StatefulCapturingBackend::new(vec![(
        200,
        serde_json::to_string(&function_response).expect("serialize function response"),
    )])
    .start_with_shutdown();
    let proxy_port = free_port();
    let config = load_agentic_config(proxy_port, model.port());
    let proxy = start_proxy(&config);

    let request = serde_json::json!({
        "model": "gpt-4.1",
        "input": "What is the weather in SF?",
        "tools": [{
            "type": "function",
            "name": "get_weather",
            "parameters": {
                "type": "object",
                "properties": {"location": {"type": "string"}}
            }
        }]
    });
    let raw = http_send(
        proxy.addr(),
        &json_post(
            "/v1/responses",
            &serde_json::to_string(&request).expect("serialize client function request"),
        ),
    );

    assert_eq!(parse_status(&raw), 200);
    let response: serde_json::Value =
        serde_json::from_str(&parse_body(&raw)).expect("client function response should be JSON");
    assert_eq!(response["id"], "resp_client_tool");
    assert_eq!(
        model.requests().len(),
        1,
        "client-side function calls must return to the client without an internal loop"
    );
}

// -----------------------------------------------------------------------------
// Round-Trip: Resolve MCP → Inference → tools/call → Inference
// -----------------------------------------------------------------------------

#[test]
fn round_trip_captures_tool_and_model_requests() {
    let first_response = serde_json::json!({
        "id": "resp_1",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_abc",
            "name": "weather__get_weather",
            "arguments": r#"{"location":"SF"}"#,
            "status": "completed"
        }]
    });
    let second_response = serde_json::json!({
        "id": "resp_2",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "The weather in SF is 72F and sunny."}]
        }]
    });

    let model = StatefulCapturingBackend::new(vec![
        (200, serde_json::to_string(&first_response).unwrap()),
        (200, serde_json::to_string(&second_response).unwrap()),
    ])
    .start_with_shutdown();

    let mcp = start_mcp_mock_server_with_config(McpMockConfig {
        tools: vec![
            McpToolFixture::new("get_weather")
                .with_description("Get the weather for a location")
                .with_input_schema(serde_json::json!({
                    "type": "object",
                    "properties": {"location": {"type": "string"}},
                    "required": ["location"],
                    "additionalProperties": false
                })),
        ],
        ..McpMockConfig::default()
    });

    let proxy_port = free_port();
    let config = load_loopback_mcp_config(proxy_port, model.port());
    let proxy = start_proxy(&config);

    let mcp_url = format!("http://127.0.0.1:{}/mcp", mcp.port());
    let request_body = serde_json::json!({
        "model": "gpt-4.1",
        "input": "What is the weather in SF?",
        "parallel_tool_calls": true,
        "tools": [{
            "type": "mcp",
            "server_label": "weather",
            "server_url": mcp_url,
            "allowed_tools": ["get_weather"],
            "require_approval": "never"
        }]
    });
    let raw = http_send(
        proxy.addr(),
        &json_post("/v1/responses", &serde_json::to_string(&request_body).unwrap()),
    );

    assert_eq!(parse_status(&raw), 200, "round-trip should return 200");
    let body = parse_body(&raw);
    let response: serde_json::Value = serde_json::from_str(&body).expect("response should be valid JSON");
    assert_eq!(
        response["id"], "resp_2",
        "final response should be the second model response"
    );

    // -------------------------------------------------------------------------
    // Assert request-supplied MCP discovery and execution
    // -------------------------------------------------------------------------
    assert!(
        mcp.method_count("tools/list") >= 1,
        "MCP resolver should call tools/list on the request-supplied server"
    );
    assert_eq!(mcp.method_count("tools/call"), 1, "MCP dispatch should call one tool");
    assert_eq!(mcp.last_tool_call_name().as_deref(), Some("get_weather"));

    let mcp_requests = mcp.received_requests();
    let call = mcp_requests
        .iter()
        .find(|request| request.json_rpc_method.as_deref() == Some("tools/call"))
        .expect("MCP server should receive tools/call");
    let call_body: serde_json::Value = serde_json::from_str(&call.body).expect("tools/call body should be JSON");
    assert_eq!(call_body["params"]["arguments"]["location"], "SF");

    // -------------------------------------------------------------------------
    // Assert resolved first request and tool-enriched second request
    // -------------------------------------------------------------------------
    let model_reqs = model.requests();
    assert_eq!(model_reqs.len(), 2, "model backend should receive exactly two requests");

    let first_model_body: serde_json::Value =
        serde_json::from_str(&model_reqs[0].body).expect("first model request body should be valid JSON");
    assert_eq!(
        first_model_body["parallel_tool_calls"], false,
        "first inference must override parallel_tool_calls=true"
    );
    let resolved_tools = first_model_body["tools"]
        .as_array()
        .expect("first model request should contain resolved tools");
    assert!(
        resolved_tools
            .iter()
            .any(|tool| tool["type"] == "function" && tool["name"] == "weather__get_weather"),
        "MCP resolver should expose the request-supplied MCP tool as an encoded function"
    );

    let second_model_req = &model_reqs[1];

    let model_body: serde_json::Value =
        serde_json::from_str(&second_model_req.body).expect("second model request body should be valid JSON");
    let input = model_body["input"]
        .as_array()
        .expect("second model request input should be an array");

    let has_function_call = input.iter().any(|item| item["type"] == "function_call");
    let has_function_call_output = input.iter().any(|item| item["type"] == "function_call_output");
    assert!(
        has_function_call,
        "second model request input should contain a function_call item"
    );
    assert!(
        has_function_call_output,
        "second model request input should contain a function_call_output item"
    );
    let function_output = input
        .iter()
        .find(|item| item["type"] == "function_call_output")
        .expect("function_call_output should be present");
    assert!(
        function_output["output"]
            .as_str()
            .is_some_and(|output| output.contains("mock result for get_weather")),
        "second inference should receive the MCP tools/call result"
    );

    // -------------------------------------------------------------------------
    // Assert openai_agentic_loop bookkeeping in second request
    // -------------------------------------------------------------------------
    assert_eq!(
        model_body["parallel_tool_calls"], false,
        "openai_agentic_loop must force parallel_tool_calls=false on re-entry"
    );
    assert_eq!(
        model_body["tool_choice"], "auto",
        "openai_agentic_loop must reset tool_choice to auto on re-entry"
    );
}

// -----------------------------------------------------------------------------
// Round-Trip: Web Search via IRR
// -----------------------------------------------------------------------------

#[test]
fn web_search_round_trip_executes_and_re_enters_inference() {
    let first_response = serde_json::json!({
        "id": "resp_ws_1",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "web_search_call",
            "id": "ws_1",
            "status": "completed",
            "action": {"type": "search", "query": "Rust 2025 edition"}
        }]
    });
    let second_response = serde_json::json!({
        "id": "resp_ws_2",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Rust 2025 brings great features."}]
        }]
    });

    let model = StatefulCapturingBackend::new(vec![
        (200, serde_json::to_string(&first_response).unwrap()),
        (200, serde_json::to_string(&second_response).unwrap()),
    ])
    .start_with_shutdown();

    let search_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let search_port = search_listener.local_addr().unwrap().port();
    spawn_search_mock(search_listener);

    let proxy_port = free_port();
    let config = load_web_search_config(proxy_port, model.port(), search_port);
    let proxy = start_proxy(&config);

    let request_body = serde_json::json!({
        "model": "gpt-4.1",
        "input": "Search for Rust 2025 edition features",
        "tools": [{"type": "web_search_preview"}]
    });
    let raw = http_send(
        proxy.addr(),
        &json_post("/v1/responses", &serde_json::to_string(&request_body).unwrap()),
    );

    assert_eq!(parse_status(&raw), 200, "web search round-trip should return 200");
    let body = parse_body(&raw);
    let response: serde_json::Value = serde_json::from_str(&body).expect("response should be JSON");
    assert_eq!(
        response["id"], "resp_ws_2",
        "final response should be the second model response after web search"
    );

    let model_reqs = model.requests();
    assert_eq!(
        model_reqs.len(),
        2,
        "model backend should receive exactly two requests (initial + post-search)"
    );

    let second_body: serde_json::Value =
        serde_json::from_str(&model_reqs[1].body).expect("second model request should be valid JSON");
    let input = second_body["input"]
        .as_array()
        .expect("second model request input should be an array");
    let has_search_result = input.iter().any(|item| item["type"] == "web_search_call");
    assert!(
        has_search_result,
        "second inference input should contain web_search_call result"
    );
}

// -----------------------------------------------------------------------------
// Round-Trip: Web Search callout honors a reloaded response ceiling
// -----------------------------------------------------------------------------

/// The `openai_web_search` callout must observe a `body_limits.max_response_bytes`
/// change applied by a hot config reload. `openai_web_search` is one of the
/// client-aware factories rebuilt against the shared
/// `ReloadableSubRequestClient`, so a reload that tightens the ceiling must reach
/// its search callout — the file-resolve reload tests cover the other client-aware
/// path. Under closed failure mode a callout body that exceeds the reloaded
/// ceiling is rejected with `status_on_error`.
///
/// Without the reload fix the rebuilt filter would keep the startup (large)
/// client, the callout would still succeed, and this would stay `200`.
///
/// The post-reload request re-runs the first inference (a third model response,
/// another `web_search_call`) before the tightened ceiling rejects the callout.
#[test]
fn web_search_callout_respects_reloaded_response_ceiling() {
    let search_call_response = serde_json::json!({
        "id": "resp_ws_1",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "web_search_call",
            "id": "ws_1",
            "status": "completed",
            "action": {"type": "search", "query": "Rust 2025 edition"}
        }]
    });
    let message_response = serde_json::json!({
        "id": "resp_ws_2",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "Rust 2025 brings great features."}]
        }]
    });
    let post_reload_search_call = serde_json::json!({
        "id": "resp_ws_3",
        "object": "response",
        "status": "completed",
        "output": [{
            "type": "web_search_call",
            "id": "ws_3",
            "status": "completed",
            "action": {"type": "search", "query": "Rust 2025 edition"}
        }]
    });

    let model = StatefulCapturingBackend::new(vec![
        (200, serde_json::to_string(&search_call_response).unwrap()),
        (200, serde_json::to_string(&message_response).unwrap()),
        (200, serde_json::to_string(&post_reload_search_call).unwrap()),
    ])
    .start_with_shutdown();

    let search_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let search_port = search_listener.local_addr().unwrap().port();
    spawn_sized_search_mock(search_listener, sized_search_body(WS_SEARCH_BODY_BYTES));

    let proxy_port = free_port();
    let guard = start_reloadable_proxy(&web_search_reload_yaml(
        proxy_port,
        model.port(),
        search_port,
        WS_LARGE_CEILING,
    ));

    let request_body = serde_json::json!({
        "model": "gpt-4.1",
        "input": "Search for Rust 2025 edition features",
        "tools": [{"type": "web_search_preview"}]
    });
    let request = json_post("/v1/responses", &serde_json::to_string(&request_body).unwrap());

    let before = http_send(guard.addr(), &request);
    assert_eq!(
        parse_status(&before),
        200,
        "under the large startup ceiling the search callout resolves and the loop completes"
    );
    let before_body: serde_json::Value = serde_json::from_str(&parse_body(&before)).expect("response should be JSON");
    assert_eq!(
        before_body["id"], "resp_ws_2",
        "the loop should re-enter inference and return the final model response"
    );

    guard.reload(&web_search_reload_yaml(
        proxy_port,
        model.port(),
        search_port,
        WS_SMALL_CEILING,
    ));

    let after = http_send(guard.addr(), &request);
    assert_eq!(
        parse_status(&after),
        502,
        "after tightening the ceiling the oversized search callout must be rejected"
    );
    let after_body: serde_json::Value =
        serde_json::from_str(&parse_body(&after)).expect("error response should be JSON");
    assert_eq!(
        after_body["error"]["type"].as_str(),
        Some("server_error"),
        "rejection should use the web search closed-failure error envelope"
    );
}

fn spawn_search_mock(listener: std::net::TcpListener) {
    use std::io::{Read as _, Write as _};
    let body = serde_json::json!({
        "web": {
            "results": [{
                "title": "Rust 2025 Edition",
                "url": "https://blog.rust-lang.org/2025",
                "description": "The Rust 2025 edition is here."
            }]
        }
    })
    .to_string();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0_u8; 4096];
        let _n = stream.read(&mut buf).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
}

fn load_web_search_config(proxy_port: u16, model_port: u16, search_port: u16) -> praxis_core::config::Config {
    let path = example_config_path("openai/responses/agentic-loop.yaml");
    let yaml = std::fs::read_to_string(path).expect("read agentic-loop example");
    let yaml = patch_yaml(&yaml, proxy_port, &HashMap::from([("127.0.0.1:3001", model_port)]));
    let yaml = yaml.replace(
        "api_key: ${WEB_SEARCH_API_KEY}",
        &format!("api_key: test-key\n                base_url: http://127.0.0.1:{search_port}"),
    );
    praxis_core::config::Config::from_yaml(&yaml).expect("parse web search config")
}

/// Build a reloadable agentic-loop config YAML string with an injected
/// `body_limits.max_response_bytes` ceiling, the web search provider pointed at
/// the mock, and a per-test sqlite store path for isolation.
fn web_search_reload_yaml(proxy_port: u16, model_port: u16, search_port: u16, max_response_bytes: usize) -> String {
    let path = example_config_path("openai/responses/agentic-loop.yaml");
    let yaml = std::fs::read_to_string(path).expect("read agentic-loop example");
    let yaml = patch_yaml(&yaml, proxy_port, &HashMap::from([("127.0.0.1:3001", model_port)]));
    let yaml = yaml.replace(
        "api_key: ${WEB_SEARCH_API_KEY}",
        &format!("api_key: test-key\n                base_url: http://127.0.0.1:{search_port}"),
    );
    let yaml = yaml.replace(
        "sqlite://responses.db?mode=rwc",
        &format!("sqlite://responses-ws-reload-{proxy_port}.db?mode=rwc"),
    );
    format!("body_limits:\n  max_response_bytes: {max_response_bytes}\n\n{yaml}")
}

/// A valid Brave-format search response padded to at least `min_bytes` so a
/// tightened response ceiling can reject it while a relaxed ceiling admits it.
fn sized_search_body(min_bytes: usize) -> String {
    let padding = "x".repeat(min_bytes);
    serde_json::json!({
        "web": {
            "results": [{
                "title": "Rust 2025 Edition",
                "url": "https://blog.rust-lang.org/2025",
                "description": padding
            }]
        }
    })
    .to_string()
}

/// Spawn a search mock that serves `body` on every connection. Unlike
/// [`spawn_search_mock`], it accepts repeated connections (one per callout across
/// the pre- and post-reload requests) and ignores write errors, since the client
/// aborts the read once the body exceeds a tightened ceiling.
fn spawn_sized_search_mock(listener: std::net::TcpListener, body: String) {
    use std::io::{Read as _, Write as _};
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let body = body.clone();
            std::thread::spawn(move || {
                let mut buf = [0_u8; 4096];
                let _n = stream.read(&mut buf);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _sent = stream.write_all(response.as_bytes());
            });
        }
    });
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn patch_web_search_api_key(yaml: &str) -> String {
    yaml.replace("api_key: ${WEB_SEARCH_API_KEY}", "api_key: test-key")
}

fn load_agentic_config(proxy_port: u16, model_port: u16) -> praxis_core::config::Config {
    let path = example_config_path("openai/responses/agentic-loop.yaml");
    let yaml = std::fs::read_to_string(path).expect("read agentic-loop example");
    let yaml = patch_yaml(&yaml, proxy_port, &HashMap::from([("127.0.0.1:3001", model_port)]));
    let yaml = patch_web_search_api_key(&yaml);
    praxis_core::config::Config::from_yaml(&yaml).expect("parse agentic-loop config")
}

fn load_loopback_mcp_config(proxy_port: u16, model_port: u16) -> praxis_core::config::Config {
    let path = example_config_path("openai/responses/agentic-loop.yaml");
    let yaml = std::fs::read_to_string(path).expect("read agentic-loop example");
    let yaml = patch_yaml(&yaml, proxy_port, &HashMap::from([("127.0.0.1:3001", model_port)]));
    let yaml = patch_web_search_api_key(&yaml);
    let yaml = yaml.replacen(
        "      - filter: openai_mcp_tool_resolve\n",
        "      - filter: openai_mcp_tool_resolve\n        allow_loopback: true\n",
        1,
    );
    let yaml = yaml.replacen(
        "              - filter: openai_mcp_dispatch\n",
        "              - filter: openai_mcp_dispatch\n                allow_loopback: true\n",
        1,
    );
    praxis_core::config::Config::from_yaml(&yaml).expect("parse loopback MCP config")
}
