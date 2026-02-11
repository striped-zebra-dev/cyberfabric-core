//! Service traits for OAGW Control and Data Planes
//!
//! This module defines the service interfaces that enable the SDK to interact
//! with OAGW's Control Plane in SharedProcess mode (direct function calls).

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use http::{HeaderMap, Method, StatusCode};
use std::sync::Arc;

/// Control Plane Service - Configuration management and request routing
///
/// In SharedProcess mode, the SDK makes direct function calls to this trait
/// instead of HTTP requests, achieving zero serialization overhead.
#[async_trait]
pub trait ControlPlaneService: Send + Sync {
    /// Execute proxy request through Data Plane
    ///
    /// # Arguments
    /// * `req` - The proxy request containing alias, method, path, headers, and body
    ///
    /// # Returns
    /// * `Ok(ProxyResponse)` - The response from the upstream service
    /// * `Err(Error)` - Gateway or connection error
    ///
    /// # Errors
    /// Returns error if:
    /// - Alias configuration not found
    /// - Data Plane connection fails
    /// - Request validation fails
    async fn proxy_request(&self, req: ProxyRequest) -> Result<ProxyResponse, Error>;
}

/// Proxy request from client to Control Plane
#[derive(Debug)]
pub struct ProxyRequest {
    /// Service alias (e.g., "openai", "anthropic", "unpkg")
    pub alias: String,

    /// HTTP method (GET, POST, PUT, DELETE, etc.)
    pub method: Method,

    /// Request path relative to service base URL (e.g., "/v1/chat/completions")
    pub path: String,

    /// Request headers to forward to upstream service
    pub headers: HeaderMap,

    /// Request body
    pub body: Body,
}

/// Proxy response from Control Plane to client
pub struct ProxyResponse {
    /// HTTP status code from upstream service
    pub status: StatusCode,

    /// Response headers from upstream service (including X-OAGW-Error-Source)
    pub headers: HeaderMap,

    /// Buffered response body (used when is_streaming = false)
    pub body: Bytes,

    /// Whether the response is streaming
    pub is_streaming: bool,

    /// Streaming response body (used when is_streaming = true)
    pub body_stream: Option<BoxStream<'static, Result<Bytes, Error>>>,
}

impl std::fmt::Debug for ProxyResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyResponse")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &format!("{} bytes", self.body.len()))
            .field("is_streaming", &self.is_streaming)
            .field("body_stream", &self.body_stream.as_ref().map(|_| "<stream>"))
            .finish()
    }
}

/// Request/response body
#[derive(Debug)]
pub enum Body {
    /// No body
    Empty,

    /// Buffered body
    Bytes(Bytes),
}

/// Service error type
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Configuration error (alias not found, invalid config)
    #[error("Configuration error: {0}")]
    Config(String),

    /// Connection error (Data Plane unreachable, network failure)
    #[error("Connection error: {0}")]
    Connection(String),

    /// Request validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Type alias for Arc-wrapped ControlPlaneService
///
/// This is the recommended way to store and pass ControlPlaneService instances
/// in SharedProcess mode.
pub type ControlPlaneServiceRef = Arc<dyn ControlPlaneService>;
