//! Integration tests for OagwClient using the SDK Request/Response API

use std::sync::Arc;

use http::{Method, StatusCode};
use modkit::client_hub::ClientHub;
use oagw::client::OagwClient;
use oagw::test_support::{APIKEY_AUTH_PLUGIN_ID, MockUpstream, TestCpBuilder, TestDpBuilder, build_test_app_state};
use oagw_sdk::api::ErrorSource;
use oagw_sdk::client::OagwClientApi;
use oagw_sdk::request::Request;
use oagw_sdk::{
    AuthConfig, BurstConfig, CreateRouteRequest, CreateUpstreamRequest, Endpoint, HttpMatch,
    HttpMethod, MatchRules, PathSuffixMode, RateLimitAlgorithm, RateLimitConfig, RateLimitScope,
    RateLimitStrategy, Scheme, Server, SharingMode, SustainedRate, Window,
};
use uuid::Uuid;

struct TestHarness {
    _mock: MockUpstream,
    client: OagwClient,
    tenant: Uuid,
}

/// Adapter to bridge internal domain DataPlaneService to SDK DataPlaneService
/// This is needed because integration tests can't access pub(crate) types
struct DataPlaneServiceAdapter {
    facade: Arc<dyn oagw_sdk::api::ServiceGatewayClientV1>,
}

#[async_trait::async_trait]
impl oagw_sdk::client::DataPlaneService for DataPlaneServiceAdapter {
    async fn proxy_request(
        &self,
        ctx: oagw_sdk::api::ProxyContext,
    ) -> Result<oagw_sdk::api::ProxyResponse, oagw_sdk::error::ServiceGatewayError> {
        self.facade.proxy_request(ctx).await
    }
}

async fn setup() -> TestHarness {
    let mock = MockUpstream::start().await;
    let addr = mock.addr();

    let hub = ClientHub::new();

    // Build test app state (contains both CP/DP and facade)
    let app_state = build_test_app_state(
        &hub,
        TestCpBuilder::new()
            .with_credentials(vec![("cred://openai-key".into(), "sk-test123".into())]),
        TestDpBuilder::new(),
    );

    let gateway = app_state.facade.clone();

    // Create adapter for SharedProcess mode
    let data_plane = Arc::new(DataPlaneServiceAdapter {
        facade: app_state.facade,
    });

    let tenant = Uuid::new_v4();

    // Create upstream pointing at mock server
    gateway
        .create_upstream(
            tenant,
            CreateUpstreamRequest::builder(
                Server {
                    endpoints: vec![Endpoint {
                        scheme: Scheme::Http,
                        host: "127.0.0.1".into(),
                        port: addr.port(),
                    }],
                },
                "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1",
            )
            .alias("mock-upstream")
            .auth(AuthConfig {
                plugin_type: APIKEY_AUTH_PLUGIN_ID.into(),
                sharing: SharingMode::Private,
                config: Some(serde_json::json!({
                    "header": "authorization",
                    "prefix": "Bearer ",
                    "secret_ref": "cred://openai-key"
                })),
            })
            .build(),
        )
        .await
        .unwrap();

    // Create route for /v1/chat/completions
    let upstream_id = gateway
        .resolve_upstream(tenant, "mock-upstream")
        .await
        .unwrap()
        .id;

    gateway
        .create_route(
            tenant,
            CreateRouteRequest::builder(
                upstream_id,
                MatchRules {
                    http: Some(HttpMatch {
                        methods: vec![HttpMethod::Post, HttpMethod::Get],
                        path: "/v1/chat/completions".into(),
                        query_allowlist: vec![],
                        path_suffix_mode: PathSuffixMode::Append,
                    }),
                    grpc: None,
                },
            )
            .build(),
        )
        .await
        .unwrap();

    // Create route for SSE streaming
    gateway
        .create_route(
            tenant,
            CreateRouteRequest::builder(
                upstream_id,
                MatchRules {
                    http: Some(HttpMatch {
                        methods: vec![HttpMethod::Post],
                        path: "/v1/chat/completions/stream".into(),
                        query_allowlist: vec![],
                        path_suffix_mode: PathSuffixMode::Append,
                    }),
                    grpc: None,
                },
            )
            .build(),
        )
        .await
        .unwrap();

    // Create route for error endpoints
    gateway
        .create_route(
            tenant,
            CreateRouteRequest::builder(
                upstream_id,
                MatchRules {
                    http: Some(HttpMatch {
                        methods: vec![HttpMethod::Get],
                        path: "/error".into(),
                        query_allowlist: vec![],
                        path_suffix_mode: PathSuffixMode::Append,
                    }),
                    grpc: None,
                },
            )
            .build(),
        )
        .await
        .unwrap();

    // Create OagwClient in SharedProcess mode
    let client = OagwClient::shared_process(data_plane).unwrap();

    TestHarness {
        _mock: mock,
        client,
        tenant,
    }
}

// Test: POST /v1/chat/completions with JSON body
#[tokio::test]
async fn client_chat_completion_round_trip() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": [{"role": "user", "content": "Hello"}]
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Parse response body as JSON
    let body_json: serde_json::Value = response.json().await.unwrap();
    assert!(body_json.get("id").is_some());
    assert!(body_json.get("choices").is_some());
}

// Test: Verify text response parsing
#[tokio::test]
async fn client_text_response() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Get response as text
    let text = response.text().await.unwrap();
    assert!(!text.is_empty());
}

// Test: Verify bytes response parsing
#[tokio::test]
async fn client_bytes_response() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    // Get response as bytes
    let bytes = response.bytes().await.unwrap();
    assert!(!bytes.is_empty());
}

// Test: SSE streaming using into_stream()
#[tokio::test]
async fn client_sse_streaming() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "stream": true
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions/stream")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    // Verify content-type is SSE
    let ct = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.contains("text/event-stream"), "got content-type: {ct}");

    // Use the SSE stream parser
    let mut sse_stream = response.into_sse_stream();
    let mut event_count = 0;

    while let Some(event) = sse_stream.next_event().await.unwrap() {
        event_count += 1;
        if event.data == "[DONE]" {
            break;
        }
    }

    assert!(event_count > 0, "Expected at least one SSE event");
}

// Test: Upstream 500 error - verify error_source
#[tokio::test]
async fn client_upstream_500_error_source() {
    let h = setup().await;

    let request = Request::builder()
        .method(Method::GET)
        .path("/error/500")
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(response.error_source(), ErrorSource::Upstream);
}

// Test: Nonexistent alias returns error
#[tokio::test]
async fn client_nonexistent_alias_returns_error() {
    let h = setup().await;

    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/test")
        .build()
        .unwrap();

    let result = h.client.execute("nonexistent", h.tenant, request).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        matches!(err, oagw_sdk::error::ClientError::Connection(_)),
        "Expected Connection error, got: {:?}",
        err
    );
}

// Test: Request with custom headers
#[tokio::test]
async fn client_custom_headers() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .header("X-Custom-Header", "custom-value")
        .unwrap()
        .header("X-Request-ID", "req-123")
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Test: Request with timeout
#[tokio::test]
async fn client_with_timeout() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Test: Empty body request
#[tokio::test]
async fn client_empty_body() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Test: String body request
#[tokio::test]
async fn client_string_body() {
    let h = setup().await;

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .header("Content-Type", "application/json")
        .unwrap()
        .body(r#"{"model":"gpt-4","messages":[]}"#)
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

// Test: Rate limit - verify Gateway error source
#[tokio::test]
async fn client_rate_limit_gateway_error_source() {
    let mock = MockUpstream::start().await;
    let addr = mock.addr();

    let hub = ClientHub::new();
    let app_state = build_test_app_state(&hub, TestCpBuilder::new(), TestDpBuilder::new());
    let gateway = app_state.facade.clone();

    // Create adapter for SharedProcess mode
    let data_plane = Arc::new(DataPlaneServiceAdapter {
        facade: app_state.facade,
    });

    let tenant = Uuid::new_v4();

    // Create upstream with tight rate limit
    let upstream = gateway
        .create_upstream(
            tenant,
            CreateUpstreamRequest::builder(
                Server {
                    endpoints: vec![Endpoint {
                        scheme: Scheme::Http,
                        host: "127.0.0.1".into(),
                        port: addr.port(),
                    }],
                },
                "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1",
            )
            .alias("rate-limited")
            .rate_limit(RateLimitConfig {
                sharing: SharingMode::Private,
                algorithm: RateLimitAlgorithm::TokenBucket,
                sustained: SustainedRate {
                    rate: 1,
                    window: Window::Minute,
                },
                burst: Some(BurstConfig { capacity: 1 }),
                scope: RateLimitScope::Tenant,
                strategy: RateLimitStrategy::Reject,
                cost: 1,
            })
            .build(),
        )
        .await
        .unwrap();

    gateway
        .create_route(
            tenant,
            CreateRouteRequest::builder(
                upstream.id,
                MatchRules {
                    http: Some(HttpMatch {
                        methods: vec![HttpMethod::Get],
                        path: "/v1/models".into(),
                        query_allowlist: vec![],
                        path_suffix_mode: PathSuffixMode::Append,
                    }),
                    grpc: None,
                },
            )
            .build(),
        )
        .await
        .unwrap();

    // Create client
    let client = OagwClient::shared_process(data_plane).unwrap();

    // First request succeeds
    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/models")
        .build()
        .unwrap();

    let response = client.execute("rate-limited", tenant, request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Second request is rate limited - should get Connection error wrapping rate limit
    let request = Request::builder()
        .method(Method::GET)
        .path("/v1/models")
        .build()
        .unwrap();

    let result = client.execute("rate-limited", tenant, request).await;
    assert!(
        result.is_err(),
        "Expected rate limit error"
    );
}

// Test: Response headers are accessible
#[tokio::test]
async fn client_response_headers() {
    let h = setup().await;

    let body = serde_json::json!({
        "model": "gpt-4",
        "messages": []
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/v1/chat/completions")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    let response = h
        .client
        .execute("mock-upstream", h.tenant, request)
        .await
        .unwrap();

    // Check headers are present
    let headers = response.headers();
    assert!(headers.get("content-type").is_some());
}

// Test: Multiple sequential requests
#[tokio::test]
async fn client_multiple_sequential_requests() {
    let h = setup().await;

    for i in 0..3 {
        let body = serde_json::json!({
            "model": "gpt-4",
            "messages": [{"role": "user", "content": format!("Message {}", i)}]
        });

        let request = Request::builder()
            .method(Method::POST)
            .path("/v1/chat/completions")
            .json(&body)
            .unwrap()
            .build()
            .unwrap();

        let response = h
            .client
            .execute("mock-upstream", h.tenant, request)
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

// Test: Builder pattern validation - missing path
#[test]
fn client_request_builder_missing_path_fails() {
    let result = Request::builder()
        .method(Method::GET)
        .build();

    assert!(result.is_err());
}

// Test: Builder pattern - method chaining
#[test]
fn client_request_builder_method_chaining() {
    let request = Request::builder()
        .method(Method::POST)
        .path("/test")
        .header("Content-Type", "application/json")
        .unwrap()
        .timeout(std::time::Duration::from_secs(10))
        .body("test body")
        .build()
        .unwrap();

    assert_eq!(request.method(), &Method::POST);
    assert_eq!(request.path(), "/test");
    assert_eq!(request.timeout(), Some(std::time::Duration::from_secs(10)));
}
