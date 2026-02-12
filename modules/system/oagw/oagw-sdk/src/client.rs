use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::{Bytes, BytesMut};
use futures::stream::{Stream, StreamExt};
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

// Re-export ErrorSource from service module (already defined there)
pub use crate::service::ErrorSource;
use crate::service::DataPlaneService;

// Type aliases for convenience
type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

// ===========================================================================
// Error Types
// ===========================================================================

/// Client errors
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("Request build error: {0}")]
    BuildError(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {status}")]
    Http { status: StatusCode, body: Bytes },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ===========================================================================
// Body Abstraction
// ===========================================================================

/// HTTP request/response body
pub enum Body {
    Empty,
    Bytes(Bytes),
    Stream(BoxStream<'static, Result<Bytes, std::io::Error>>),
}

impl Body {
    /// Create an empty body
    pub fn empty() -> Self {
        Body::Empty
    }

    /// Create a body from bytes
    pub fn from_bytes(bytes: impl Into<Bytes>) -> Self {
        Body::Bytes(bytes.into())
    }

    /// Create a body from JSON value
    pub fn from_json<T: Serialize>(value: &T) -> Result<Self, serde_json::Error> {
        Ok(Body::Bytes(serde_json::to_vec(value)?.into()))
    }

    /// Create a body from a stream
    pub fn from_stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    {
        Body::Stream(Box::pin(stream))
    }

    /// Convert body to bytes (consumes the body)
    pub async fn into_bytes(self) -> Result<Bytes, ClientError> {
        match self {
            Body::Empty => Ok(Bytes::new()),
            Body::Bytes(b) => Ok(b),
            Body::Stream(mut s) => {
                let mut buf = BytesMut::new();
                while let Some(chunk) = s.next().await {
                    buf.extend_from_slice(&chunk?);
                }
                Ok(buf.freeze())
            }
        }
    }
}

impl fmt::Debug for Body {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Body::Empty => write!(f, "Body::Empty"),
            Body::Bytes(b) => write!(f, "Body::Bytes({} bytes)", b.len()),
            Body::Stream(_) => write!(f, "Body::Stream"),
        }
    }
}

impl From<()> for Body {
    fn from(_: ()) -> Self {
        Body::Empty
    }
}

impl From<Bytes> for Body {
    fn from(b: Bytes) -> Self {
        Body::Bytes(b)
    }
}

impl From<Vec<u8>> for Body {
    fn from(v: Vec<u8>) -> Self {
        Body::Bytes(v.into())
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Body::Bytes(s.into())
    }
}

impl From<&str> for Body {
    fn from(s: &str) -> Self {
        Body::Bytes(Bytes::from(s.to_owned()))
    }
}

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
// Response Types
// ===========================================================================

/// Internal response body representation
pub(crate) enum ResponseBody {
    Buffered(Bytes),
    Streaming(BoxStream<'static, Result<Bytes, ClientError>>),
}

/// HTTP response
pub struct Response {
    pub(crate) status: StatusCode,
    pub(crate) headers: HeaderMap,
    pub(crate) body: ResponseBody,
    pub(crate) error_source: ErrorSource,
}

impl Response {
    /// Create a new Response from streaming body
    pub fn from_stream(
        status: StatusCode,
        headers: HeaderMap,
        stream: BoxStream<'static, Result<Bytes, ClientError>>,
        error_source: ErrorSource,
    ) -> Self {
        Self {
            status,
            headers,
            body: ResponseBody::Streaming(stream),
            error_source,
        }
    }

    /// Create a new Response from buffered bytes
    pub fn from_bytes(
        status: StatusCode,
        headers: HeaderMap,
        bytes: Bytes,
        error_source: ErrorSource,
    ) -> Self {
        Self {
            status,
            headers,
            body: ResponseBody::Buffered(bytes),
            error_source,
        }
    }

    /// Get response status code
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// Get response headers
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get error source (gateway vs upstream)
    pub fn error_source(&self) -> ErrorSource {
        self.error_source
    }

    /// Buffer entire response body
    pub async fn bytes(self) -> Result<Bytes, ClientError> {
        match self.body {
            ResponseBody::Buffered(bytes) => Ok(bytes),
            ResponseBody::Streaming(mut stream) => {
                let mut buf = BytesMut::new();
                while let Some(chunk) = stream.next().await {
                    buf.extend_from_slice(&chunk?);
                }
                Ok(buf.freeze())
            }
        }
    }

    /// Parse response body as JSON
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, ClientError> {
        let bytes = self.bytes().await?;
        serde_json::from_slice(&bytes).map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    /// Parse response body as text
    pub async fn text(self) -> Result<String, ClientError> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes.to_vec()).map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    /// Consume response as byte stream (for SSE, chunked responses)
    pub fn into_stream(self) -> BoxStream<'static, Result<Bytes, ClientError>> {
        match self.body {
            ResponseBody::Buffered(bytes) => {
                Box::pin(futures::stream::once(async move { Ok(bytes) }))
            }
            ResponseBody::Streaming(stream) => stream,
        }
    }

    /// Convenience: parse as Server-Sent Events stream
    pub fn into_sse_stream(self) -> SseEventStream {
        SseEventStream::new(self.into_stream())
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("error_source", &self.error_source)
            .finish()
    }
}

// ===========================================================================
// Server-Sent Events (SSE) Types
// ===========================================================================

/// Server-Sent Events stream parser
pub struct SseEventStream {
    inner: BoxStream<'static, Result<Bytes, ClientError>>,
    buffer: Vec<u8>,
}

impl SseEventStream {
    /// Create a new SSE event stream
    pub fn new(stream: BoxStream<'static, Result<Bytes, ClientError>>) -> Self {
        Self {
            inner: stream,
            buffer: Vec::new(),
        }
    }

    /// Get the next SSE event
    pub async fn next_event(&mut self) -> Result<Option<SseEvent>, ClientError> {
        loop {
            // Check if buffer contains complete event
            if let Some(event) = self.parse_buffered_event()? {
                return Ok(Some(event));
            }

            // Read more data
            match self.inner.next().await {
                Some(Ok(chunk)) => {
                    self.buffer.extend_from_slice(&chunk);
                }
                Some(Err(e)) => return Err(e),
                None => {
                    // Stream ended
                    if self.buffer.is_empty() {
                        return Ok(None);
                    } else {
                        // Return partial data as final event
                        return self.parse_buffered_event();
                    }
                }
            }
        }
    }

    fn parse_buffered_event(&mut self) -> Result<Option<SseEvent>, ClientError> {
        // Find double newline (event separator)
        if let Some(pos) = self.buffer.windows(2).position(|w| w == b"\n\n") {
            let event_bytes: Vec<u8> = self.buffer.drain(..pos + 2).collect();
            return Ok(Some(Self::parse_sse_event(&event_bytes)?));
        }
        Ok(None)
    }

    fn parse_sse_event(data: &[u8]) -> Result<SseEvent, ClientError> {
        let mut id = None;
        let mut event = None;
        let mut data_lines = Vec::new();
        let mut retry = None;

        for line in data.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }

            // Parse "field: value" format
            if let Some(colon_pos) = line.iter().position(|&b| b == b':') {
                let field = &line[..colon_pos];
                let value = &line[colon_pos + 1..];

                // Skip optional space after colon
                let value = if value.first() == Some(&b' ') {
                    &value[1..]
                } else {
                    value
                };

                match field {
                    b"id" => id = Some(String::from_utf8_lossy(value).to_string()),
                    b"event" => event = Some(String::from_utf8_lossy(value).to_string()),
                    b"data" => data_lines.push(String::from_utf8_lossy(value).to_string()),
                    b"retry" => {
                        retry = String::from_utf8_lossy(value).parse().ok();
                    }
                    _ => {} // Ignore unknown fields
                }
            }
        }

        Ok(SseEvent {
            id,
            event,
            data: data_lines.join("\n"),
            retry,
        })
    }
}

/// Server-Sent Event
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub id: Option<String>,
    pub event: Option<String>,
    pub data: String,
    pub retry: Option<u64>,
}

// ===========================================================================
// WebSocket Types (Phase 1)
// ===========================================================================

use tokio::sync::mpsc;

/// WebSocket connection
pub struct WebSocketConn {
    send: mpsc::Sender<WsMessage>,
    recv: mpsc::Receiver<Result<WsMessage, ClientError>>,
}

impl WebSocketConn {
    /// Create a new WebSocket connection
    pub fn new(
        send: mpsc::Sender<WsMessage>,
        recv: mpsc::Receiver<Result<WsMessage, ClientError>>,
    ) -> Self {
        Self { send, recv }
    }

    /// Send a WebSocket message
    pub async fn send(&mut self, msg: WsMessage) -> Result<(), ClientError> {
        self.send
            .send(msg)
            .await
            .map_err(|_| ClientError::ConnectionClosed)
    }

    /// Receive a WebSocket message
    pub async fn recv(&mut self) -> Result<Option<WsMessage>, ClientError> {
        self.recv.recv().await.transpose()
    }

    /// Close the WebSocket connection
    pub async fn close(self) -> Result<(), ClientError> {
        drop(self.send);
        Ok(())
    }
}

/// WebSocket message types
#[derive(Debug, Clone)]
pub enum WsMessage {
    Text(String),
    Binary(Bytes),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<CloseFrame>),
}

/// WebSocket close frame
#[derive(Debug, Clone)]
pub struct CloseFrame {
    pub code: u16,
    pub reason: String,
}

// ===========================================================================
// Client Configuration
// ===========================================================================

/// Client configuration
#[derive(Clone)]
pub struct OagwClientConfig {
    pub mode: ClientMode,
    pub default_timeout: Duration,
}

/// Deployment mode
#[derive(Clone)]
pub enum ClientMode {
    /// OAGW in same process - direct function calls
    SharedProcess {
        data_plane: Arc<dyn DataPlaneService>,
    },

    /// OAGW in separate process - HTTP proxy
    RemoteProxy {
        base_url: String,
        auth_token: String,
        timeout: Duration,
    },
}

impl OagwClientConfig {
    /// Create configuration for SharedProcess mode
    pub fn shared_process(data_plane: Arc<dyn DataPlaneService>) -> Self {
        Self {
            mode: ClientMode::SharedProcess { data_plane },
            default_timeout: Duration::from_secs(30),
        }
    }

    /// Create configuration for RemoteProxy mode
    pub fn remote_proxy(base_url: String, auth_token: String) -> Self {
        Self {
            mode: ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout: Duration::from_secs(30),
            },
            default_timeout: Duration::from_secs(30),
        }
    }
}

// ===========================================================================
// Client API Trait
// ===========================================================================

/// Public client API for making HTTP requests through OAGW
///
/// Consuming modules create their own OagwClient instances using `OagwClient::from_ctx()`.
#[async_trait]
pub trait OagwClientApi: Send + Sync {
    /// Execute HTTP request through OAGW
    ///
    /// The response can be consumed as buffered or streaming:
    /// - Buffered: `response.bytes()`, `response.json()`, `response.text()`
    /// - Streaming: `response.into_stream()`, `response.into_sse_stream()`
    ///
    /// # Arguments
    ///
    /// * `alias` - Upstream alias (e.g., "openai", "anthropic")
    /// * `request` - HTTP request to execute
    async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError>;

    /// Establish WebSocket connection through OAGW (Phase 1)
    async fn websocket(&self, alias: &str, request: Request) -> Result<WebSocketConn, ClientError>;
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_builder() {
        let request = Request::builder()
            .method(Method::POST)
            .path("/v1/chat/completions")
            .build()
            .unwrap();

        assert_eq!(request.method(), &Method::POST);
        assert_eq!(request.path(), "/v1/chat/completions");
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
    fn test_body_from_string() {
        let body = Body::from("test");
        match body {
            Body::Bytes(b) => assert_eq!(b, "test"),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[tokio::test]
    async fn test_body_into_bytes() {
        let body = Body::from("test");
        let bytes = body.into_bytes().await.unwrap();
        assert_eq!(bytes, "test");
    }

    #[test]
    fn test_sse_event_parsing() {
        let data = b"id: 123\nevent: message\ndata: hello\ndata: world\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.id, Some("123".to_string()));
        assert_eq!(event.event, Some("message".to_string()));
        assert_eq!(event.data, "hello\nworld");
    }

    #[test]
    fn test_client_error_display() {
        let err = ClientError::BuildError("test error".into());
        assert_eq!(err.to_string(), "Request build error: test error");
    }
}
