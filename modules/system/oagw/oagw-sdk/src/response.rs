use std::fmt;
use std::pin::Pin;

use bytes::{Bytes, BytesMut};
use futures::stream::{Stream, StreamExt};
use http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;

use crate::error::ClientError;
use crate::service::ErrorSource;
use crate::sse::SseEventStream;

// Type aliases for convenience
type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

// ===========================================================================
// Response Types
// ===========================================================================

/// Internal response body representation
pub enum ResponseBody {
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
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn test_response_from_bytes() {
        let status = StatusCode::OK;
        let headers = HeaderMap::new();
        let body = Bytes::from("test response");
        let error_source = ErrorSource::Upstream;

        let response = Response::from_bytes(status, headers.clone(), body.clone(), error_source);

        assert_eq!(response.status(), status);
        assert_eq!(response.headers(), &headers);
        assert_eq!(response.error_source(), error_source);
    }

    #[test]
    fn test_response_from_stream() {
        let status = StatusCode::OK;
        let headers = HeaderMap::new();
        let stream = Box::pin(stream::iter(vec![Ok(Bytes::from("test"))]));
        let error_source = ErrorSource::Gateway;

        let response = Response::from_stream(status, headers.clone(), stream, error_source);

        assert_eq!(response.status(), status);
        assert_eq!(response.headers(), &headers);
        assert_eq!(response.error_source(), error_source);
    }

    #[tokio::test]
    async fn test_response_bytes_from_buffered() {
        let body = Bytes::from("test data");
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body.clone(),
            ErrorSource::Upstream,
        );

        let result = response.bytes().await.unwrap();
        assert_eq!(result, body);
    }

    #[tokio::test]
    async fn test_response_bytes_from_streaming() {
        let chunks = vec![
            Ok(Bytes::from("hello ")),
            Ok(Bytes::from("world")),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let response = Response::from_stream(
            StatusCode::OK,
            HeaderMap::new(),
            stream,
            ErrorSource::Upstream,
        );

        let result = response.bytes().await.unwrap();
        assert_eq!(result, "hello world");
    }

    #[tokio::test]
    async fn test_response_json() {
        let data = serde_json::json!({"key": "value", "number": 42});
        let body = Bytes::from(serde_json::to_vec(&data).unwrap());
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body,
            ErrorSource::Upstream,
        );

        let result: serde_json::Value = response.json().await.unwrap();
        assert_eq!(result, data);
    }

    #[tokio::test]
    async fn test_response_json_error() {
        let body = Bytes::from("not valid json");
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body,
            ErrorSource::Upstream,
        );

        let result: Result<serde_json::Value, _> = response.json().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClientError::InvalidResponse(_)));
    }

    #[tokio::test]
    async fn test_response_text() {
        let body = Bytes::from("hello world");
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body,
            ErrorSource::Upstream,
        );

        let result = response.text().await.unwrap();
        assert_eq!(result, "hello world");
    }

    #[tokio::test]
    async fn test_response_text_invalid_utf8() {
        let body = Bytes::from(vec![0xFF, 0xFE, 0xFD]);
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body,
            ErrorSource::Upstream,
        );

        let result = response.text().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClientError::InvalidResponse(_)));
    }

    #[tokio::test]
    async fn test_response_into_stream_from_buffered() {
        let body = Bytes::from("test data");
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            body.clone(),
            ErrorSource::Upstream,
        );

        let mut stream = response.into_stream();
        let chunk = stream.next().await.unwrap().unwrap();
        assert_eq!(chunk, body);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_response_into_stream_from_streaming() {
        let chunks = vec![
            Ok(Bytes::from("chunk1")),
            Ok(Bytes::from("chunk2")),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let response = Response::from_stream(
            StatusCode::OK,
            HeaderMap::new(),
            stream,
            ErrorSource::Upstream,
        );

        let mut result_stream = response.into_stream();
        let chunk1 = result_stream.next().await.unwrap().unwrap();
        let chunk2 = result_stream.next().await.unwrap().unwrap();
        assert_eq!(chunk1, "chunk1");
        assert_eq!(chunk2, "chunk2");
        assert!(result_stream.next().await.is_none());
    }

    #[test]
    fn test_response_status() {
        let response = Response::from_bytes(
            StatusCode::NOT_FOUND,
            HeaderMap::new(),
            Bytes::new(),
            ErrorSource::Gateway,
        );

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_response_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );

        let response = Response::from_bytes(
            StatusCode::OK,
            headers.clone(),
            Bytes::new(),
            ErrorSource::Upstream,
        );

        assert_eq!(
            response.headers().get(http::header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn test_response_error_source() {
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::new(),
            ErrorSource::Gateway,
        );

        assert_eq!(response.error_source(), ErrorSource::Gateway);
    }

    #[test]
    fn test_response_debug() {
        let response = Response::from_bytes(
            StatusCode::OK,
            HeaderMap::new(),
            Bytes::from("test"),
            ErrorSource::Upstream,
        );

        let debug = format!("{:?}", response);
        assert!(debug.contains("Response"));
        assert!(debug.contains("200"));
    }

    #[tokio::test]
    async fn test_response_into_sse_stream() {
        let sse_data = b"data: hello\n\n";
        let stream = Box::pin(stream::once(async { Ok(Bytes::from(&sse_data[..])) }));
        let response = Response::from_stream(
            StatusCode::OK,
            HeaderMap::new(),
            stream,
            ErrorSource::Upstream,
        );

        let mut sse_stream = response.into_sse_stream();
        let event = sse_stream.next_event().await.unwrap();
        assert!(event.is_some());
    }
}
