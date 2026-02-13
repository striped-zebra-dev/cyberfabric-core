use std::time::Duration;

use http::{HeaderMap, HeaderName, HeaderValue, Method};
use serde::Serialize;

use crate::body::Body;
use crate::error::ClientError;

// ===========================================================================
// Request Types
// ===========================================================================

/// HTTP request
#[derive(Debug)]
pub struct Request {
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Body,
    timeout: Option<Duration>,
}

impl Request {
    /// Create a new request builder
    pub fn builder() -> RequestBuilder {
        RequestBuilder::new()
    }

    /// Get the HTTP method
    pub fn method(&self) -> &Method {
        &self.method
    }

    /// Get the request path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get request headers
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get mutable request headers
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get request body reference
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Consume request and return body
    pub fn into_body(self) -> Body {
        self.body
    }

    /// Get request timeout
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

/// HTTP request builder
#[derive(Debug)]
pub struct RequestBuilder {
    method: Method,
    path: Option<String>,
    headers: HeaderMap,
    body: Option<Body>,
    timeout: Option<Duration>,
}

impl RequestBuilder {
    /// Create a new request builder
    pub fn new() -> Self {
        Self {
            method: Method::GET,
            path: None,
            headers: HeaderMap::new(),
            body: None,
            timeout: None,
        }
    }

    /// Set HTTP method
    pub fn method(mut self, method: Method) -> Self {
        self.method = method;
        self
    }

    /// Set request path
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Add a header
    pub fn header<K, V>(mut self, key: K, value: V) -> Result<Self, ClientError>
    where
        K: TryInto<HeaderName>,
        V: TryInto<HeaderValue>,
        K::Error: std::error::Error + Send + Sync + 'static,
        V::Error: std::error::Error + Send + Sync + 'static,
    {
        let key = key
            .try_into()
            .map_err(|e| ClientError::BuildError(format!("Invalid header name: {}", e)))?;
        let value = value
            .try_into()
            .map_err(|e| ClientError::BuildError(format!("Invalid header value: {}", e)))?;
        self.headers.insert(key, value);
        Ok(self)
    }

    /// Set request body
    pub fn body<B: Into<Body>>(mut self, body: B) -> Self {
        self.body = Some(body.into());
        self
    }

    /// Set JSON request body
    pub fn json<T: Serialize>(mut self, value: &T) -> Result<Self, ClientError> {
        let body = Body::from_json(value)?;
        self.headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        self.body = Some(body);
        Ok(self)
    }

    /// Set request timeout
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Build the request
    pub fn build(self) -> Result<Request, ClientError> {
        let path = self
            .path
            .ok_or_else(|| ClientError::BuildError("Missing request path".into()))?;

        Ok(Request {
            method: self.method,
            path,
            headers: self.headers,
            body: self.body.unwrap_or(Body::Empty),
            timeout: self.timeout,
        })
    }
}

impl Default for RequestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder_default() {
        let builder = RequestBuilder::default();
        let request = builder.path("/test").build().unwrap();
        assert_eq!(request.method(), &Method::GET);
        assert_eq!(request.path(), "/test");
    }

    #[test]
    fn test_request_builder() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/v1/chat/completions")
            .build()
            .unwrap();

        assert_eq!(request.method(), &Method::POST);
        assert_eq!(request.path(), "/v1/chat/completions");
        assert!(matches!(request.body(), Body::Empty));
    }

    #[test]
    fn test_request_builder_with_body() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/test")
            .body("test data")
            .build()
            .unwrap();

        match request.body() {
            Body::Bytes(b) => assert_eq!(b, "test data"),
            _ => panic!("Expected Bytes body"),
        }
    }

    #[test]
    fn test_request_builder_json() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/test")
            .json(&serde_json::json!({"key": "value"}))
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_request_builder_with_headers() {
        let request = Request::builder()
            .method(Method::GET)
            .path("/test")
            .header("X-Custom-Header", "custom-value")
            .unwrap()
            .header("Authorization", "Bearer token")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            request.headers().get("X-Custom-Header").unwrap(),
            "custom-value"
        );
        assert_eq!(
            request.headers().get("Authorization").unwrap(),
            "Bearer token"
        );
    }

    #[test]
    fn test_request_builder_with_timeout() {
        let timeout = Duration::from_secs(60);
        let request = Request::builder()
            .method(Method::GET)
            .path("/test")
            .timeout(timeout)
            .build()
            .unwrap();

        assert_eq!(request.timeout(), Some(timeout));
    }

    #[test]
    fn test_request_builder_missing_path() {
        let result = Request::builder()
            .method(Method::GET)
            .build();

        assert!(result.is_err());
        match result {
            Err(ClientError::BuildError(msg)) => {
                assert!(msg.contains("Missing request path"));
            }
            _ => panic!("Expected BuildError"),
        }
    }

    #[test]
    fn test_request_headers_mut() {
        let mut request = Request::builder()
            .method(Method::GET)
            .path("/test")
            .build()
            .unwrap();

        request.headers_mut().insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("text/plain"),
        );

        assert_eq!(
            request.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "text/plain"
        );
    }

    #[test]
    fn test_request_into_body() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/test")
            .body("test data")
            .build()
            .unwrap();

        let body = request.into_body();
        match body {
            Body::Bytes(b) => assert_eq!(b, "test data"),
            _ => panic!("Expected Bytes body"),
        }
    }

    #[test]
    fn test_request_debug() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/test")
            .build()
            .unwrap();

        let debug = format!("{:?}", request);
        assert!(debug.contains("Request"));
    }

    #[test]
    fn test_request_builder_method_chaining() {
        let request = Request::builder()
            .method(Method::PUT)
            .path("/api/resource")
            .body("data")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();

        assert_eq!(request.method(), &Method::PUT);
        assert_eq!(request.path(), "/api/resource");
        assert_eq!(request.timeout(), Some(Duration::from_secs(30)));
    }
}
