// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Unit tests for the MCP client wrapper.

use std::time::Duration;

use super::*;

const TEST_TIMEOUT: Duration = Duration::from_secs(5);

async fn validate_url(url: &str) -> Result<(), McpClientError> {
    validate_mcp_url(url, TEST_TIMEOUT, false).await
}

fn display_url(url: &str) -> McpDisplayUrl {
    McpDisplayUrl::from_uri(&url.parse().unwrap())
}

fn assert_error_uses_sanitized_url(error: &McpClientError) {
    let message = error.to_string();
    assert!(
        message.contains("https://example.com:8443/mcp/tools"),
        "sanitized endpoint should survive in: {message}"
    );
    for secret in ["user", "pass", "api_key", "TOPSECRET"] {
        assert!(
            !message.contains(secret),
            "credential fragment {secret:?} leaked into error: {message}"
        );
    }
}

// =========================================================================
// Transport Config
// =========================================================================

#[test]
fn build_config_with_no_headers() {
    let config = build_transport_config("http://localhost:8001/mcp", None, None).unwrap();
    assert_eq!(&*config.uri, "http://localhost:8001/mcp", "URI should match");
    assert!(config.custom_headers.is_empty(), "no custom headers expected");
}

#[test]
fn build_config_with_headers() {
    let headers = serde_json::json!({"x-custom": "value", "x-other": "val2"});
    let config = build_transport_config("http://localhost:8001/mcp", Some(&headers), None).unwrap();

    assert_eq!(config.custom_headers.len(), 2, "should have 2 custom headers");
}

#[test]
fn build_config_ignores_non_string_header_values() {
    let headers = serde_json::json!({"x-good": "ok", "x-bad": 123});
    let config = build_transport_config("http://localhost:8001/mcp", Some(&headers), None).unwrap();

    assert_eq!(
        config.custom_headers.len(),
        1,
        "should only include string-valued headers"
    );
}

#[test]
fn build_config_ignores_non_object_headers() {
    let headers = serde_json::json!("not-an-object");
    let config = build_transport_config("http://localhost:8001/mcp", Some(&headers), None).unwrap();

    assert!(config.custom_headers.is_empty(), "non-object headers should be ignored");
}

// =========================================================================
// Hop-by-hop / framing header blocking
// =========================================================================

#[test]
fn hop_by_hop_headers_stripped_from_mcp_headers() {
    let headers = serde_json::json!({
        "host": "evil.example.com",
        "content-length": "999",
        "transfer-encoding": "chunked",
        "connection": "keep-alive",
        "te": "trailers",
        "trailer": "Foo",
        "upgrade": "websocket",
        "proxy-authorization": "Basic creds",
        "x-custom": "safe"
    });
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), None).unwrap();

    assert_eq!(config.custom_headers.len(), 1, "only safe header should remain");
    assert!(
        config
            .custom_headers
            .contains_key(&http::HeaderName::from_static("x-custom")),
        "x-custom should pass through"
    );
}

#[test]
fn reserved_internal_headers_stripped_from_mcp_headers() {
    let headers = serde_json::json!({
        "x-praxis-ai-format": "openai",
        "x-mcp-servername": "backend-1",
        "x-a2a-method": "task/send",
        "x-custom": "safe"
    });
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), None).unwrap();

    assert_eq!(config.custom_headers.len(), 1, "only safe header should remain");
    assert!(
        config
            .custom_headers
            .contains_key(&http::HeaderName::from_static("x-custom")),
        "x-custom should pass through"
    );
}

// =========================================================================
// Authorization
// =========================================================================

#[test]
fn authorization_injects_bearer_header() {
    let config = build_transport_config("http://api.example.com/mcp", None, Some("tok_abc")).unwrap();
    let auth = config.custom_headers.get(&http::header::AUTHORIZATION).unwrap();
    assert_eq!(auth, "Bearer tok_abc", "should inject Bearer token");
}

#[test]
fn authorization_with_custom_headers() {
    let headers = serde_json::json!({"x-custom": "val"});
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), Some("tok_xyz")).unwrap();

    assert_eq!(config.custom_headers.len(), 2, "should have both headers");
    assert_eq!(
        config.custom_headers.get(&http::header::AUTHORIZATION).unwrap(),
        "Bearer tok_xyz",
        "should have authorization"
    );
}

#[test]
fn authorization_field_overrides_headers_authorization() {
    let headers = serde_json::json!({"authorization": "Basic creds"});
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), Some("tok_real")).unwrap();

    let auth = config.custom_headers.get(&http::header::AUTHORIZATION).unwrap();
    assert_eq!(
        auth, "Bearer tok_real",
        "authorization field should win over headers.Authorization"
    );
}

#[test]
fn authorization_in_headers_stripped_when_no_field() {
    let headers = serde_json::json!({"authorization": "Basic creds", "x-custom": "val"});
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), None).unwrap();

    assert!(
        !config.custom_headers.contains_key(&http::header::AUTHORIZATION),
        "Authorization from headers should be stripped"
    );
    assert_eq!(config.custom_headers.len(), 1, "only x-custom should remain");
}

#[test]
fn no_authorization_no_header() {
    let config = build_transport_config("http://api.example.com/mcp", None, None).unwrap();
    assert!(config.custom_headers.is_empty(), "no headers expected");
}

#[test]
fn authorization_with_invalid_chars_returns_error() {
    let result = build_transport_config("http://api.example.com/mcp", None, Some("tok\x00bad"));
    assert!(result.is_err(), "invalid header chars should return error");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("invalid HTTP header"),
        "error should describe invalid header: {msg}"
    );
}

// =========================================================================
// Error Display
// =========================================================================

#[test]
fn connection_error_display() {
    let err = McpClientError::Connection {
        url: display_url("http://example.com/mcp"),
    };
    let msg = err.to_string();
    assert!(msg.contains("example.com"), "should include URL");
    assert!(msg.contains("connection failed"), "should describe failure");
}

#[test]
fn timeout_error_display() {
    let err = McpClientError::Timeout {
        url: display_url("http://example.com/mcp"),
        timeout: Duration::from_secs(5),
    };
    let msg = err.to_string();
    assert!(msg.contains("timed out"), "should describe timeout");
    assert!(msg.contains("5s"), "should include duration");
}

#[test]
fn too_many_tools_error_display() {
    let err = McpClientError::TooManyTools {
        url: display_url("http://example.com/mcp"),
        count: 200,
        max: 128,
    };
    let msg = err.to_string();
    assert!(msg.contains("200"), "should include actual count");
    assert!(msg.contains("128"), "should include max limit");
}

#[test]
fn list_tools_error_display() {
    let err = McpClientError::ListTools {
        url: display_url("http://example.com/mcp"),
    };
    let msg = err.to_string();
    assert!(msg.contains("tools/list failed"), "should describe failure");
    assert!(msg.contains("example.com"), "should include URL");
}

#[test]
fn invalid_authorization_error_display() {
    let err = McpClientError::InvalidAuthorization;
    let msg = err.to_string();
    assert!(
        msg.contains("invalid HTTP header"),
        "should describe invalid header: {msg}"
    );
}

#[test]
fn display_url_keeps_locators_and_drops_secrets() {
    // scheme + host + port + path survive; userinfo, query, and fragment do not.
    let cases = [
        (
            "https://user:pass@example.com:8443/mcp/tools?api_key=TOPSECRET#frag",
            "https://example.com:8443/mcp/tools",
        ),
        ("http://example.com/", "http://example.com/"),
        ("https://token@host.example/path", "https://host.example/path"),
    ];
    for (raw, expected) in cases {
        assert_eq!(display_url(raw).to_string(), expected, "input: {raw}");
    }
}

#[test]
fn display_url_brackets_ipv6_and_strips_credentials() {
    assert_eq!(
        display_url("https://user:pass@[2001:db8::1]:8443/mcp?api_key=TOPSECRET").to_string(),
        "https://[2001:db8::1]:8443/mcp",
    );
    // Bare IPv6 host, no port: brackets are still restored.
    assert_eq!(display_url("http://[fd00::5]/x").to_string(), "http://[fd00::5]/x");
}

#[test]
fn every_url_bearing_error_variant_is_sanitized() {
    let url = display_url("https://user:pass@example.com:8443/mcp/tools?api_key=TOPSECRET");
    let variants = [
        McpClientError::Connection { url: url.clone() },
        McpClientError::ListTools { url: url.clone() },
        McpClientError::Timeout {
            url: url.clone(),
            timeout: Duration::from_secs(5),
        },
        McpClientError::TooManyTools {
            url: url.clone(),
            count: 200,
            max: 128,
        },
        McpClientError::SsrfBlocked {
            url,
            reason: "test reason",
        },
    ];

    for variant in &variants {
        assert_error_uses_sanitized_url(variant);
    }
}

// =========================================================================
// SSRF Validation
// =========================================================================

#[tokio::test]
async fn ssrf_blocks_ipv4_loopback() {
    assert!(validate_url("http://127.0.0.1/mcp").await.is_err());
    assert!(validate_url("http://127.0.0.99:8080/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_ipv6_loopback() {
    assert!(validate_url("http://[::1]/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_ipv6_link_local() {
    assert!(validate_url("http://[fe80::1]/mcp").await.is_err());
    assert!(validate_url("http://[fe80::1%25eth0]:8080/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_localhost_hostname() {
    assert!(validate_url("http://localhost/mcp").await.is_err());
    assert!(validate_url("http://LOCALHOST/mcp").await.is_err());
    assert!(validate_url("http://sub.localhost/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_link_local() {
    assert!(validate_url("http://169.254.169.254/latest/meta-data/").await.is_err());
    assert!(validate_url("http://169.254.0.1/mcp").await.is_err());
}

#[tokio::test]
async fn alibaba_metadata_ipv4_is_blocked() {
    assert!(
        validate_url("http://100.100.100.200/latest/meta-data/").await.is_err(),
        "Alibaba Cloud metadata IPv4 must be treated as SSRF"
    );
}

#[tokio::test]
async fn ssrf_blocks_mapped_ipv4_loopback() {
    assert!(validate_url("http://[::ffff:127.0.0.1]/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_mapped_metadata() {
    assert!(validate_url("http://[::ffff:169.254.169.254]/mcp").await.is_err());
}

#[tokio::test]
async fn alibaba_metadata_ipv4_mapped_ipv6_is_blocked() {
    assert!(
        validate_url("http://[::ffff:100.100.100.200]/latest/meta-data/").await.is_err(),
        "IPv4-mapped Alibaba metadata address must be normalized then blocked"
    );
}

#[test]
fn alibaba_metadata_via_dns_is_blocked() {
    let resolved = ["100.100.100.200:80".parse::<SocketAddr>().unwrap()];
    let shown = display_url("http://metadata.example/mcp");
    assert!(
        check_resolved_addrs(&resolved, &shown, false).is_err(),
        "a hostname resolving to Alibaba metadata must be blocked after DNS"
    );
}

#[tokio::test]
async fn ssrf_blocks_invalid_url() {
    assert!(validate_url("not-a-url").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_unresolvable_hostname() {
    assert!(validate_url("http://unresolvable.invalid/mcp").await.is_err());
}

#[tokio::test]
async fn blocked_url_errors_hide_query_and_fragment() {
    let with_secrets = [
        "http://unresolvable.invalid/mcp?api_key=TOPSECRET",
        "http://127.0.0.1/admin?token=TOPSECRET",
        "http://169.254.169.254/latest?token=TOPSECRET#FRAGMENTSECRET",
    ];

    for raw in with_secrets {
        let message = validate_url(raw).await.unwrap_err().to_string();
        for leaked in ["TOPSECRET", "FRAGMENTSECRET"] {
            assert!(!message.contains(leaked), "{leaked} leaked from {raw}: {message}");
        }
    }
}

#[tokio::test]
async fn unshowable_urls_use_opaque_placeholder() {
    let malformed = [
        "http://exa mple.com/mcp?api_key=TOPSECRET#FRAGMENTSECRET",
        "//user:pass@example.com/mcp?api_key=TOPSECRET#FRAGMENTSECRET",
        "ftp://user:pass@example.com/mcp?api_key=TOPSECRET#FRAGMENTSECRET",
    ];

    for raw in malformed {
        let message = validate_url(raw).await.unwrap_err().to_string();
        assert!(
            message.contains("<invalid MCP URL>"),
            "expected opaque placeholder for {raw}: {message}"
        );
        for secret in ["user", "pass", "TOPSECRET", "FRAGMENTSECRET"] {
            assert!(!message.contains(secret), "{secret} leaked from {raw}: {message}");
        }
    }
}

#[tokio::test]
async fn blocked_urls_report_actionable_reason() {
    let expectations = [
        ("ftp://example.com/mcp", "scheme must be http or https"),
        ("http://user:pass@example.com/mcp", "embedded credentials are not allowed"),
        ("http://localhost/mcp", "localhost hostnames are not allowed"),
        (
            "http://127.0.0.1/mcp",
            "address is loopback, link-local, unspecified, or cloud metadata",
        ),
    ];

    for (raw, reason) in expectations {
        let message = validate_url(raw).await.unwrap_err().to_string();
        assert!(message.contains(reason), "missing actionable reason for {raw}: {message}");
    }
}

#[tokio::test]
async fn ssrf_allows_public_ips() {
    assert!(validate_url("http://8.8.8.8/mcp").await.is_ok());
    assert!(validate_url("https://1.1.1.1:443/v1").await.is_ok());
}

#[tokio::test]
async fn ssrf_allows_private_rfc1918() {
    assert!(validate_url("http://10.0.0.5/mcp").await.is_ok());
    assert!(validate_url("http://192.168.1.100/mcp").await.is_ok());
}

#[test]
fn ssrf_blocked_display_lists_ssrf_url_and_reason() {
    let err = McpClientError::SsrfBlocked {
        url: display_url("http://127.0.0.1/mcp"),
        reason: "loopback address is not allowed",
    };
    let msg = err.to_string();
    for expected in ["SSRF", "127.0.0.1", "loopback address is not allowed"] {
        assert!(msg.contains(expected), "display missing {expected:?}: {msg}");
    }
}

#[tokio::test]
async fn ssrf_blocks_unspecified_ipv4() {
    assert!(validate_url("http://0.0.0.0/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_unspecified_ipv6() {
    assert!(validate_url("http://[::]/mcp").await.is_err());
}

#[tokio::test]
async fn ssrf_blocks_mapped_unspecified() {
    assert!(validate_url("http://[::ffff:0.0.0.0]/mcp").await.is_err());
}

#[tokio::test]
async fn userinfo_urls_are_blocked_and_redacted() {
    let msg = validate_url("http://user:pass@example.com/mcp").await.unwrap_err().to_string();
    assert!(!msg.contains("pass"), "userinfo password must not leak: {msg}");
    assert!(validate_url("https://user@example.com/mcp").await.is_err());

    let ipv6 = validate_url("http://user:pass@[::1]:8080/mcp?api_key=TOPSECRET")
        .await
        .unwrap_err()
        .to_string();
    for secret in ["user", "pass", "TOPSECRET"] {
        assert!(!ipv6.contains(secret), "IPv6 error leaked {secret}: {ipv6}");
    }
}

#[tokio::test]
async fn ssrf_blocks_aws_imds_ipv6() {
    assert!(validate_url("http://[fd00:ec2::254]/latest/meta-data/").await.is_err());
}

#[test]
fn aws_imds_v6_detected_by_is_ssrf_sensitive() {
    let ip = "fd00:ec2::254".parse::<IpAddr>().unwrap();
    assert!(is_ssrf_sensitive(&ip), "fd00:ec2::254 should be SSRF-sensitive");
}

#[test]
fn unspecified_ip_detected_by_is_ssrf_sensitive() {
    let v4 = "0.0.0.0".parse::<IpAddr>().unwrap();
    assert!(is_ssrf_sensitive(&v4), "0.0.0.0 should be SSRF-sensitive");
    let v6 = "::".parse::<IpAddr>().unwrap();
    assert!(is_ssrf_sensitive(&v6), ":: should be SSRF-sensitive");
}

#[test]
fn no_authorization_field_injects_no_auth_header() {
    let headers = serde_json::json!({"x-custom": "val"});
    let config = build_transport_config("http://api.example.com/mcp", Some(&headers), None).unwrap();
    assert!(
        !config.custom_headers.contains_key(&http::header::AUTHORIZATION),
        "should not inject Authorization when authorization field is absent"
    );
}

#[test]
fn ipv6_link_local_detected_by_is_ssrf_sensitive() {
    let fe80 = "fe80::1".parse::<IpAddr>().unwrap();
    assert!(is_ssrf_sensitive(&fe80), "fe80::1 should be SSRF-sensitive");
    let febf = "febf::1".parse::<IpAddr>().unwrap();
    assert!(is_ssrf_sensitive(&febf), "febf::1 should be SSRF-sensitive");
    let fe00 = "fe00::1".parse::<IpAddr>().unwrap();
    assert!(!is_ssrf_sensitive(&fe00), "fe00::1 is not link-local");
}

// =========================================================================
// allow_loopback
// =========================================================================

#[tokio::test]
async fn allow_loopback_permits_ipv4_loopback() {
    assert!(
        validate_mcp_url("http://127.0.0.1/mcp", TEST_TIMEOUT, true)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn allow_loopback_permits_localhost_hostname() {
    assert!(
        validate_mcp_url("http://localhost/mcp", TEST_TIMEOUT, true)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn allow_loopback_still_blocks_link_local() {
    assert!(
        validate_mcp_url("http://169.254.169.254/mcp", TEST_TIMEOUT, true)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn allow_loopback_still_blocks_unspecified() {
    assert!(
        validate_mcp_url("http://0.0.0.0/mcp", TEST_TIMEOUT, true)
            .await
            .is_err()
    );
}
