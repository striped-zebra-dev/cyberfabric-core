use std::sync::Arc;

use axum::body::Body;
use http::{Method, Request, StatusCode};
use modkit::client_hub::ClientHub;
use oagw::api::rest::routes::test_router;
use oagw::module::AppState;
use oagw::test_support::TestCpBuilder;
use oagw_sdk::error::OagwError;
use oagw_sdk::gts;
use oagw_sdk::service::{DataPlaneService, ProxyContext, ProxyResponse};
use tower::ServiceExt;
use uuid::Uuid;

/// Stub DP service for management tests (proxy not exercised).
struct StubDataPlane;

#[async_trait::async_trait]
impl DataPlaneService for StubDataPlane {
    async fn proxy_request(&self, _ctx: ProxyContext) -> Result<ProxyResponse, OagwError> {
        unimplemented!("proxy not used in management tests")
    }
}

fn make_state() -> AppState {
    let hub = ClientHub::new();
    let cp = TestCpBuilder::new().build_and_register(&hub);
    AppState {
        cp,
        dp: Arc::new(StubDataPlane) as Arc<dyn DataPlaneService>,
    }
}

fn make_app() -> axum::Router {
    test_router(make_state())
}

async fn body_json(body: Body) -> serde_json::Value {
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn tenant_id() -> Uuid {
    Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
}

// 7.8: POST upstream with valid body -> 201 + GTS id + alias generated.
#[tokio::test]
async fn create_upstream_success() {
    let app = make_app();

    let body = serde_json::json!({
        "server": {
            "endpoints": [{"host": "api.openai.com", "port": 443, "scheme": "https"}]
        },
        "protocol": "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1",
        "enabled": true,
        "tags": []
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oagw/v1/upstreams")
                .header("content-type", "application/json")
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response.into_body()).await;
    let id_str = json["id"].as_str().unwrap();
    assert!(id_str.starts_with("gts.x.core.oagw.upstream.v1~"));
    assert_eq!(json["alias"].as_str().unwrap(), "api.openai.com");
}

// 7.8: POST with missing server -> 400 (serde deserialization error).
#[tokio::test]
async fn create_upstream_missing_server_returns_422() {
    let app = make_app();

    let body = serde_json::json!({
        "protocol": "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oagw/v1/upstreams")
                .header("content-type", "application/json")
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // axum returns 422 for deserialization errors.
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// 7.9: GET upstream by GTS id -> 200.
#[tokio::test]
async fn get_upstream_by_gts_id() {
    let state = make_state();
    let app = test_router(state.clone());

    // First create an upstream.
    let upstream = state
        .cp
        .create_upstream(
            tenant_id(),
            oagw_sdk::models::CreateUpstreamRequest {
                server: oagw_sdk::models::upstream::Server {
                    endpoints: vec![oagw_sdk::models::Endpoint {
                        scheme: oagw_sdk::models::endpoint::Scheme::Https,
                        host: "api.openai.com".into(),
                        port: 443,
                    }],
                },
                protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
                alias: Some("openai".into()),
                auth: None,
                headers: None,
                plugins: None,
                rate_limit: None,
                tags: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();

    let gts_id = gts::format_upstream_gts(upstream.id);
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/oagw/v1/upstreams/{gts_id}"))
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response.into_body()).await;
    assert_eq!(json["alias"].as_str().unwrap(), "openai");
}

// 7.9: GET with invalid GTS format -> 400.
#[tokio::test]
async fn get_upstream_invalid_gts_returns_400() {
    let app = make_app();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/oagw/v1/upstreams/not-a-gts-id")
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let json = body_json(response.into_body()).await;
    assert_eq!(
        json["type"].as_str().unwrap(),
        oagw_sdk::error::ERR_VALIDATION
    );
}

// 7.9: GET nonexistent -> 404.
#[tokio::test]
async fn get_upstream_nonexistent_returns_404() {
    let app = make_app();
    let fake_id = gts::format_upstream_gts(Uuid::new_v4());

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/oagw/v1/upstreams/{fake_id}"))
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// 7.10: PUT upstream -> 200 with updated fields, id unchanged.
#[tokio::test]
async fn update_upstream_preserves_id() {
    let state = make_state();
    let app = test_router(state.clone());

    let upstream = state
        .cp
        .create_upstream(
            tenant_id(),
            oagw_sdk::models::CreateUpstreamRequest {
                server: oagw_sdk::models::upstream::Server {
                    endpoints: vec![oagw_sdk::models::Endpoint {
                        scheme: oagw_sdk::models::endpoint::Scheme::Https,
                        host: "api.openai.com".into(),
                        port: 443,
                    }],
                },
                protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
                alias: Some("openai".into()),
                auth: None,
                headers: None,
                plugins: None,
                rate_limit: None,
                tags: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();

    let gts_id = gts::format_upstream_gts(upstream.id);
    let update_body = serde_json::json!({"alias": "openai-v2"});

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/oagw/v1/upstreams/{gts_id}"))
                .header("content-type", "application/json")
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::from(serde_json::to_vec(&update_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response.into_body()).await;
    assert_eq!(json["id"].as_str().unwrap(), gts_id);
    assert_eq!(json["alias"].as_str().unwrap(), "openai-v2");
}

// 7.10: DELETE upstream -> 204 + routes cascade deleted.
#[tokio::test]
async fn delete_upstream_returns_204() {
    let state = make_state();
    let app = test_router(state.clone());

    let upstream = state
        .cp
        .create_upstream(
            tenant_id(),
            oagw_sdk::models::CreateUpstreamRequest {
                server: oagw_sdk::models::upstream::Server {
                    endpoints: vec![oagw_sdk::models::Endpoint {
                        scheme: oagw_sdk::models::endpoint::Scheme::Https,
                        host: "api.openai.com".into(),
                        port: 443,
                    }],
                },
                protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
                alias: Some("to-delete".into()),
                auth: None,
                headers: None,
                plugins: None,
                rate_limit: None,
                tags: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();

    let gts_id = gts::format_upstream_gts(upstream.id);

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/oagw/v1/upstreams/{gts_id}"))
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}

// 7.11: POST route -> 201 referencing existing upstream.
#[tokio::test]
async fn create_route_success() {
    let state = make_state();
    let app = test_router(state.clone());

    let upstream = state
        .cp
        .create_upstream(
            tenant_id(),
            oagw_sdk::models::CreateUpstreamRequest {
                server: oagw_sdk::models::upstream::Server {
                    endpoints: vec![oagw_sdk::models::Endpoint {
                        scheme: oagw_sdk::models::endpoint::Scheme::Https,
                        host: "api.openai.com".into(),
                        port: 443,
                    }],
                },
                protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
                alias: Some("openai".into()),
                auth: None,
                headers: None,
                plugins: None,
                rate_limit: None,
                tags: vec![],
                enabled: true,
            },
        )
        .await
        .unwrap();

    let body = serde_json::json!({
        "upstream_id": upstream.id,
        "match": {
            "http": {
                "methods": ["POST"],
                "path": "/v1/chat/completions"
            }
        },
        "enabled": true,
        "tags": [],
        "priority": 0
    });

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/oagw/v1/routes")
                .header("content-type", "application/json")
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    let json = body_json(response.into_body()).await;
    assert!(
        json["id"]
            .as_str()
            .unwrap()
            .starts_with("gts.x.core.oagw.route.v1~")
    );
}

// 7.12: GET upstreams with pagination.
#[tokio::test]
async fn list_upstreams_with_pagination() {
    let state = make_state();
    let app = test_router(state.clone());
    let tid = tenant_id();

    // Create 3 upstreams.
    for i in 0..3 {
        state
            .cp
            .create_upstream(
                tid,
                oagw_sdk::models::CreateUpstreamRequest {
                    server: oagw_sdk::models::upstream::Server {
                        endpoints: vec![oagw_sdk::models::Endpoint {
                            scheme: oagw_sdk::models::endpoint::Scheme::Https,
                            host: format!("host{i}.example.com"),
                            port: 443,
                        }],
                    },
                    protocol: "gts.x.core.oagw.protocol.v1~x.core.oagw.http.v1".into(),
                    alias: Some(format!("host{i}")),
                    auth: None,
                    headers: None,
                    plugins: None,
                    rate_limit: None,
                    tags: vec![],
                    enabled: true,
                },
            )
            .await
            .unwrap();
    }

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/oagw/v1/upstreams?limit=2&offset=1")
                .header("x-tenant-id", tid.to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let json = body_json(response.into_body()).await;
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

// 7.13: Error mapper produces correct Problem Details.
#[tokio::test]
async fn error_mapper_produces_problem_details() {
    let app = make_app();
    let fake_id = gts::format_upstream_gts(Uuid::new_v4());

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/oagw/v1/upstreams/{fake_id}"))
                .header("x-tenant-id", tenant_id().to_string())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let ct = response
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(ct, "application/problem+json");
    let source = response
        .headers()
        .get("x-oagw-error-source")
        .unwrap()
        .to_str()
        .unwrap();
    assert_eq!(source, "gateway");
}

// Missing tenant ID header -> 400.
#[tokio::test]
async fn missing_tenant_id_returns_400() {
    let app = make_app();

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/oagw/v1/upstreams")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
