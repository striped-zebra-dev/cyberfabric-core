//! HTTP request types

use super::Body;
use crate::error::ClientError;
use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method};
use std::time::Duration;

/// HTTP request to be proxied through OAGW
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
    #[must_use]
    pub fn builder() -> RequestBuilder {
        RequestBuilder::new()
    }

    /// Get the HTTP method
    #[must_use]
    pub const fn method(&self) -> &Method {
        &self.method
    }

    /// Get the request path
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Get the request headers
    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get a mutable reference to the request headers
    #[must_use]
    pub fn headers_mut(&mut self) -> &mut HeaderMap {
        &mut self.headers
    }

    /// Get the request timeout
    #[must_use]
    pub const fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Consume the request and return the body
    #[must_use]
    pub fn into_body(self) -> Body {
        self.body
    }

    /// Get a reference to the body
    #[must_use]
    pub const fn body(&self) -> &Body {
        &self.body
    }
}

/// Builder for constructing HTTP requests
#[derive(Debug, Default)]
pub struct RequestBuilder {
    method: Option<Method>,
    path: Option<String>,
    headers: HeaderMap,
    body: Option<Body>,
    timeout: Option<Duration>,
}

impl RequestBuilder {
    /// Create a new request builder
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the HTTP method
    #[must_use]
    pub fn method(mut self, method: Method) -> Self {
        self.method = Some(method);
        self
    }

    /// Set the request path (relative to service base URL)
    ///
    /// # Example
    /// ```ignore
    /// Request::builder()
    ///     .path("/v1/chat/completions")
    ///     .build()
    /// ```
    #[must_use]
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Add a header to the request
    ///
    /// # Errors
    /// Returns error if header name or value is invalid
    pub fn header(mut self, name: HeaderName, value: HeaderValue) -> Result<Self, ClientError> {
        self.headers.insert(name, value);
        Ok(self)
    }

    /// Add a header to the request (panics on invalid header)
    ///
    /// # Panics
    /// Panics if header name or value is invalid
    #[must_use]
    pub fn header_str(mut self, name: &str, value: &str) -> Self {
        self.headers.insert(
            HeaderName::try_from(name).expect("invalid header name"),
            HeaderValue::try_from(value).expect("invalid header value"),
        );
        self
    }

    /// Set the request body from bytes
    #[must_use]
    pub fn body(mut self, body: Body) -> Self {
        self.body = Some(body);
        self
    }

    /// Set the request body from bytes
    #[must_use]
    pub fn body_bytes(mut self, bytes: Bytes) -> Self {
        self.body = Some(Body::from_bytes(bytes));
        self
    }

    /// Set the request body from a string
    #[must_use]
    pub fn body_string(mut self, s: String) -> Self {
        self.body = Some(Body::from(s));
        self
    }

    /// Set the request body as JSON
    ///
    /// # Errors
    /// Returns error if serialization fails
    pub fn json<T: serde::Serialize>(mut self, value: &T) -> Result<Self, ClientError> {
        let bytes = serde_json::to_vec(value).map_err(|e| {
            ClientError::BuildError(format!("Failed to serialize JSON: {e}"))
        })?;
        self.body = Some(Body::from_vec(bytes));
        self.headers.insert(
            http::header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        Ok(self)
    }

    /// Set the request timeout
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Build the request
    ///
    /// # Errors
    /// Returns error if required fields are missing
    pub fn build(self) -> Result<Request, ClientError> {
        let method = self
            .method
            .ok_or_else(|| ClientError::BuildError("Method is required".into()))?;
        let path = self
            .path
            .ok_or_else(|| ClientError::BuildError("Path is required".into()))?;

        // Ensure path starts with /
        let path = if path.starts_with('/') {
            path
        } else {
            format!("/{path}")
        };

        Ok(Request {
            method,
            path,
            headers: self.headers,
            body: self.body.unwrap_or_default(),
            timeout: self.timeout,
        })
    }
}

// Convenience constructors
impl RequestBuilder {
    /// Create a GET request builder
    #[must_use]
    pub fn get() -> Self {
        Self::new().method(Method::GET)
    }

    /// Create a POST request builder
    #[must_use]
    pub fn post() -> Self {
        Self::new().method(Method::POST)
    }

    /// Create a PUT request builder
    #[must_use]
    pub fn put() -> Self {
        Self::new().method(Method::PUT)
    }

    /// Create a DELETE request builder
    #[must_use]
    pub fn delete() -> Self {
        Self::new().method(Method::DELETE)
    }

    /// Create a PATCH request builder
    #[must_use]
    pub fn patch() -> Self {
        Self::new().method(Method::PATCH)
    }
}
