//! Integration tests for RemoteProxyClient
//!
//! These tests use httpmock to simulate OAGW HTTP endpoints

use bytes::Bytes;
use httpmock::prelude::*;
use oagw_sdk::{ClientMode, ErrorSource, Method, OagwClient, OagwClientConfig, Request};
use std::time::Duration;

#[tokio::test]
async fn test_remote_proxy_buffered_request() {
    let server = MockServer::start();

    // Mock OAGW proxy endpoint
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/openai/v1/models");
        then.status(200)
            .header("x-oagw-error-source", "upstream")
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "data": [
                    {"id": "gpt-4", "object": "model"},
                    {"id": "gpt-3.5-turbo", "object": "model"}
                ]
            }));
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/models")
        .build()
        .unwrap();

    let response = client.execute("openai", request).await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.error_source(), ErrorSource::Upstream);
    assert!(response.is_success());

    let data: serde_json::Value = response.json().await.unwrap();
    assert_eq!(data["data"].as_array().unwrap().len(), 2);

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_post_with_json() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/api/oagw/v1/proxy/openai/v1/chat/completions")
            .header("authorization", "Bearer test-token")
            .header("content-type", "application/json")
            .json_body(serde_json::json!({
                "model": "gpt-4",
                "messages": [{"role": "user", "content": "Hello"}]
            }));
        then.status(200)
            .header("x-oagw-error-source", "upstream")
            .json_body(serde_json::json!({
                "choices": [{
                    "message": {"content": "Hi!"}
                }]
            }));
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": "Hello"}]
        }))
        .unwrap()
        .build()
        .unwrap();

    let response = client.execute("openai", request).await.unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.error_source(), ErrorSource::Upstream);

    let data: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        data["choices"][0]["message"]["content"],
        "Hi!"
    );

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_gateway_error() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/invalid/v1/test");
        then.status(404)
            .header("x-oagw-error-source", "gateway")
            .body("Alias not found");
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/test")
        .build()
        .unwrap();

    let response = client.execute("invalid", request).await.unwrap();

    assert_eq!(response.status(), 404);
    assert_eq!(response.error_source(), ErrorSource::Gateway);
    assert!(response.is_gateway_error());
    assert!(!response.is_upstream_error());

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_upstream_error() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/openai/v1/error");
        then.status(429)
            .header("x-oagw-error-source", "upstream")
            .json_body(serde_json::json!({
                "error": {
                    "message": "Rate limit exceeded",
                    "type": "rate_limit_error"
                }
            }));
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/error")
        .build()
        .unwrap();

    let response = client.execute("openai", request).await.unwrap();

    assert_eq!(response.status(), 429);
    assert_eq!(response.error_source(), ErrorSource::Upstream);
    assert!(response.is_upstream_error());
    assert!(!response.is_gateway_error());

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_streaming_response() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/test/stream");
        then.status(200)
            .header("content-type", "text/event-stream")
            .body("data: chunk1\n\ndata: chunk2\n\ndata: chunk3\n\n");
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/stream")
        .build()
        .unwrap();

    let response = client.execute("test", request).await.unwrap();

    assert_eq!(response.status(), 200);

    // Convert to SSE stream
    let mut sse_stream = response.into_sse_stream();

    let event1 = sse_stream.next_event().await.unwrap().unwrap();
    assert_eq!(event1.data, "chunk1");

    let event2 = sse_stream.next_event().await.unwrap().unwrap();
    assert_eq!(event2.data, "chunk2");

    let event3 = sse_stream.next_event().await.unwrap().unwrap();
    assert_eq!(event3.data, "chunk3");

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_custom_headers() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/api/test")
            .header("x-custom-header", "custom-value")
            .header("x-api-key", "secret-key");
        then.status(200)
            .body("OK");
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/test")
        .header_str("x-custom-header", "custom-value")
        .header_str("x-api-key", "secret-key")
        .build()
        .unwrap();

    let response = client.execute("api", request).await.unwrap();

    assert_eq!(response.status(), 200);

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_text_response() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/api/text");
        then.status(200)
            .header("content-type", "text/plain")
            .body("Hello, World!");
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/text")
        .build()
        .unwrap();

    let response = client.execute("api", request).await.unwrap();

    assert_eq!(response.status(), 200);

    let text = response.text().await.unwrap();
    assert_eq!(text, "Hello, World!");

    mock.assert();
}

#[tokio::test]
async fn test_remote_proxy_bytes_response() {
    let server = MockServer::start();

    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/oagw/v1/proxy/api/binary");
        then.status(200)
            .header("content-type", "application/octet-stream")
            .body(vec![0x01, 0x02, 0x03, 0x04]);
    });

    let config = OagwClientConfig {
        mode: ClientMode::RemoteProxy {
            base_url: server.base_url(),
            auth_token: "test-token".to_string(),
            timeout: Duration::from_secs(10),
        },
        default_timeout: Duration::from_secs(10),
    };

    let client = OagwClient::from_config(config).unwrap();

    let request = Request::builder()
        .method(Method::GET)
        .path("/binary")
        .build()
        .unwrap();

    let response = client.execute("api", request).await.unwrap();

    assert_eq!(response.status(), 200);

    let bytes = response.bytes().await.unwrap();
    assert_eq!(bytes, Bytes::from_static(&[0x01, 0x02, 0x03, 0x04]));

    mock.assert();
}
