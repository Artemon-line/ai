// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Functional reload test for `body_limits.max_response_bytes`.

use std::{
    io::{Read as _, Write as _},
    net::{TcpListener, TcpStream},
    time::Duration,
};

use praxis_test_utils::{free_port, http_send, json_post, parse_body, parse_status, start_reloadable_proxy};

const PROBE_FILE_ID: &str = "reload-probe";

const PROBE_METADATA: &str =
    r#"{"id":"reload-probe","object":"file","content_type":"text/plain","filename":"probe.txt","purpose":"user_data"}"#;

const LARGE_CEILING: usize = 65_536;

const SMALL_CEILING: usize = 1_024;

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[test]
fn tightening_ceiling_on_reload_rejects_oversized_file() {
    let files_api_port = start_files_api_stub();
    let inference_port = start_inference_backend();
    let proxy_port = free_port();

    let guard = start_reloadable_proxy(&config_yaml(proxy_port, files_api_port, inference_port, LARGE_CEILING));

    let before = http_send(guard.addr(), &probe_request());
    assert_eq!(
        parse_status(&before),
        200,
        "with the large startup ceiling the file should resolve and forward"
    );

    guard.reload(&config_yaml(proxy_port, files_api_port, inference_port, SMALL_CEILING));

    let after = http_send(guard.addr(), &probe_request());
    assert_eq!(
        parse_status(&after),
        413,
        "after tightening the ceiling the oversized content callout must be rejected"
    );
    let error: serde_json::Value =
        serde_json::from_str(&parse_body(&after)).expect("error response should be valid JSON");
    assert_eq!(
        error["error"]["type"].as_str(),
        Some("file_resolve_error"),
        "rejection should use the file resolution error envelope"
    );
}

#[test]
fn relaxing_ceiling_on_reload_admits_previously_rejected_file() {
    let files_api_port = start_files_api_stub();
    let inference_port = start_inference_backend();
    let proxy_port = free_port();

    let guard = start_reloadable_proxy(&config_yaml(proxy_port, files_api_port, inference_port, SMALL_CEILING));

    let before = http_send(guard.addr(), &probe_request());
    assert_eq!(
        parse_status(&before),
        413,
        "with the small startup ceiling the oversized content callout should be rejected"
    );

    guard.reload(&config_yaml(proxy_port, files_api_port, inference_port, LARGE_CEILING));

    let after = http_send(guard.addr(), &probe_request());
    assert_eq!(
        parse_status(&after),
        200,
        "after relaxing the ceiling the previously rejected file should resolve"
    );
}

// -----------------------------------------------------------------------------
// Test Utilities
// -----------------------------------------------------------------------------

fn probe_content() -> String {
    "A".repeat(4096)
}

fn config_yaml(proxy_port: u16, files_api_port: u16, inference_port: u16, max_response_bytes: usize) -> String {
    format!(
        r#"
body_limits:
  max_response_bytes: {max_response_bytes}

listeners:
  - name: ai-gateway
    address: "127.0.0.1:{proxy_port}"
    filter_chains: [file-resolve-pipeline]

filter_chains:
  - name: file-resolve-pipeline
    filters:
      - filter: openai_responses_format
        on_invalid: continue
        headers:
          format: x-praxis-ai-format
          model: x-praxis-ai-model

      - filter: openai_file_resolve
        files_api_url: "http://127.0.0.1:{files_api_port}"
        allow_private_files_api_url: true
        allow_pre_security_callout: true
        on_missing: reject
        timeout_ms: 10000

      - filter: router
        routes:
          - path: "/v1/responses"
            cluster: "inference-backend"

      - filter: load_balancer
        clusters:
          - name: "inference-backend"
            endpoints:
              - "127.0.0.1:{inference_port}"
"#
    )
}

fn probe_request() -> String {
    let body = format!(
        r#"{{
            "model": "gpt-4.1",
            "input": [
                {{
                    "type": "message",
                    "role": "user",
                    "content": [
                        {{"type": "input_file", "file_id": "{PROBE_FILE_ID}"}}
                    ]
                }}
            ]
        }}"#
    );
    json_post("/v1/responses", &body)
}

fn start_inference_backend() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || respond(stream, 200, "application/json", b"{}"));
        }
    });
    port
}

fn start_files_api_stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle_files_api_request(stream));
        }
    });
    port
}

fn handle_files_api_request(mut stream: TcpStream) {
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();

    let mut data = Vec::new();
    let mut buf = [0_u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&buf[..n]),
        }
        if data.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }

    let raw = String::from_utf8_lossy(&data);
    let path = raw
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("/");

    if path.ends_with("/content") && path.contains(PROBE_FILE_ID) {
        respond(stream, 200, "text/plain", probe_content().as_bytes());
    } else if path.contains(PROBE_FILE_ID) {
        respond(stream, 200, "application/json", PROBE_METADATA.as_bytes());
    } else {
        respond(stream, 404, "application/json", br#"{"error":"not found"}"#);
    }
}

fn respond(mut stream: TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _sent = stream.write_all(header.as_bytes());
    let _sent = stream.write_all(body);
}
