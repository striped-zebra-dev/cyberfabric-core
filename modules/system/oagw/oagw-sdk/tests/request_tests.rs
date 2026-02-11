//! Tests for Request and RequestBuilder

use bytes::Bytes;
use oagw_sdk::{Body, Method, Request};
use serde_json::json;

#[test]
fn test_build_simple_request() {
    let request = Request::builder()
        .method(Method::GET)
        .path("/test")
        .build()
        .unwrap();

    assert_eq!(request.method(), &Method::GET);
    assert_eq!(request.path(), "/test");
    assert!(request.headers().is_empty());
    assert!(request.timeout().is_none());
}

#[test]
fn test_build_request_with_path_normalization() {
    // Path without leading slash should be normalized
    let request = Request::builder()
        .method(Method::GET)
        .path("test")
        .build()
        .unwrap();

    assert_eq!(request.path(), "/test");
}

#[test]
fn test_build_request_with_path_already_normalized() {
    let request = Request::builder()
        .method(Method::GET)
        .path("/test")
        .build()
        .unwrap();

    assert_eq!(request.path(), "/test");
}

#[test]
fn test_build_request_with_headers() {
    let request = Request::builder()
        .method(Method::POST)
        .path("/api/test")
        .header_str("content-type", "application/json")
        .header_str("x-custom-header", "custom-value")
        .build()
        .unwrap();

    assert_eq!(request.headers().len(), 2);
    assert_eq!(
        request.headers().get("content-type").unwrap(),
        "application/json"
    );
    assert_eq!(
        request.headers().get("x-custom-header").unwrap(),
        "custom-value"
    );
}

#[test]
fn test_build_request_with_json_body() {
    let body = json!({
        "key": "value",
        "number": 42
    });

    let request = Request::builder()
        .method(Method::POST)
        .path("/api/data")
        .json(&body)
        .unwrap()
        .build()
        .unwrap();

    // Should have content-type header
    assert_eq!(
        request.headers().get("content-type").unwrap(),
        "application/json"
    );

    // Body should be present
    assert!(!request.body().is_empty());
}

#[test]
fn test_build_request_with_bytes_body() {
    let body_data = Bytes::from_static(b"test data");

    let request = Request::builder()
        .method(Method::POST)
        .path("/upload")
        .body_bytes(body_data.clone())
        .build()
        .unwrap();

    match request.body() {
        Body::Bytes(bytes) => assert_eq!(bytes, &body_data),
        _ => panic!("Expected Bytes body"),
    }
}

#[test]
fn test_build_request_with_string_body() {
    let request = Request::builder()
        .method(Method::POST)
        .path("/text")
        .body_string("hello world".to_string())
        .build()
        .unwrap();

    match request.body() {
        Body::Bytes(bytes) => assert_eq!(bytes, &Bytes::from("hello world")),
        _ => panic!("Expected Bytes body"),
    }
}

#[test]
fn test_build_request_with_timeout() {
    let timeout = std::time::Duration::from_secs(30);

    let request = Request::builder()
        .method(Method::GET)
        .path("/test")
        .timeout(timeout)
        .build()
        .unwrap();

    assert_eq!(request.timeout(), Some(timeout));
}

#[test]
fn test_build_request_missing_method() {
    let result = Request::builder().path("/test").build();

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Method"));
}

#[test]
fn test_build_request_missing_path() {
    let result = Request::builder().method(Method::GET).build();

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Path"));
}

#[test]
fn test_convenience_builders() {
    use oagw_sdk::RequestBuilder;

    let get = RequestBuilder::get().path("/test").build().unwrap();
    assert_eq!(get.method(), &Method::GET);

    let post = RequestBuilder::post().path("/test").build().unwrap();
    assert_eq!(post.method(), &Method::POST);

    let put = RequestBuilder::put().path("/test").build().unwrap();
    assert_eq!(put.method(), &Method::PUT);

    let delete = RequestBuilder::delete().path("/test").build().unwrap();
    assert_eq!(delete.method(), &Method::DELETE);

    let patch = RequestBuilder::patch().path("/test").build().unwrap();
    assert_eq!(patch.method(), &Method::PATCH);
}

#[test]
fn test_json_serialization_success() {
    let value = json!({
        "valid": "data"
    });

    // This should succeed
    let result = Request::builder()
        .method(Method::POST)
        .path("/test")
        .json(&value);

    assert!(result.is_ok());
}

#[test]
fn test_empty_body_by_default() {
    let request = Request::builder()
        .method(Method::GET)
        .path("/test")
        .build()
        .unwrap();

    assert!(request.body().is_empty());
}

#[test]
fn test_body_debug_format() {
    let empty = Body::empty();
    let debug_str = format!("{:?}", empty);
    assert_eq!(debug_str, "Empty");

    let bytes = Body::from_bytes(Bytes::from_static(b"test"));
    let debug_str = format!("{:?}", bytes);
    assert!(debug_str.contains("Bytes"));
    assert!(debug_str.contains("4 bytes"));
}
