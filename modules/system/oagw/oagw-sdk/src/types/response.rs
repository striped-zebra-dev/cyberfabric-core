//! HTTP response types

use super::ErrorSource;
use crate::error::ClientError;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::StreamExt;
use http::{HeaderMap, StatusCode};

/// HTTP response from OAGW
pub struct Response {
    status: StatusCode,
    headers: HeaderMap,
    body: ResponseBody,
    error_source: ErrorSource,
}

/// Internal response body representation
pub(crate) enum ResponseBody {
    /// Fully buffered response body
    Buffered(Bytes),

    /// Streaming response body
    Streaming(BoxStream<'static, Result<Bytes, ClientError>>),
}

impl std::fmt::Debug for ResponseBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buffered(bytes) => f
                .debug_tuple("Buffered")
                .field(&format!("{} bytes", bytes.len()))
                .finish(),
            Self::Streaming(_) => f.debug_tuple("Streaming").finish(),
        }
    }
}

impl Response {
    /// Create a new response
    #[must_use]
    pub(crate) fn new(
        status: StatusCode,
        headers: HeaderMap,
        body: ResponseBody,
        error_source: ErrorSource,
    ) -> Self {
        Self {
            status,
            headers,
            body,
            error_source,
        }
    }

    /// Get the HTTP status code
    #[must_use]
    pub const fn status(&self) -> StatusCode {
        self.status
    }

    /// Get the response headers
    #[must_use]
    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Get the error source
    #[must_use]
    pub const fn error_source(&self) -> ErrorSource {
        self.error_source
    }

    /// Check if the response was successful (2xx status code)
    #[must_use]
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Check if the response is an error (4xx or 5xx status code)
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.status.is_client_error() || self.status.is_server_error()
    }

    /// Consume the response and return the body as bytes
    ///
    /// If the response is streaming, this will buffer the entire stream.
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - I/O error occurs
    pub async fn bytes(self) -> Result<Bytes, ClientError> {
        match self.body {
            ResponseBody::Buffered(bytes) => Ok(bytes),
            ResponseBody::Streaming(mut stream) => {
                let mut chunks = Vec::new();
                while let Some(chunk) = stream.next().await {
                    chunks.push(chunk?);
                }
                Ok(chunks.into_iter().fold(Bytes::new(), |mut acc, chunk| {
                    acc = Bytes::from([acc, chunk].concat());
                    acc
                }))
            }
        }
    }

    /// Consume the response and return the body as text
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - Body is not valid UTF-8
    pub async fn text(self) -> Result<String, ClientError> {
        let bytes = self.bytes().await?;
        String::from_utf8(bytes.to_vec())
            .map_err(|e| ClientError::InvalidResponse(format!("Invalid UTF-8: {e}")))
    }

    /// Consume the response and deserialize the body as JSON
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - Body is not valid JSON
    /// - Deserialization fails
    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, ClientError> {
        let bytes = self.bytes().await?;
        serde_json::from_slice(&bytes)
            .map_err(|e| ClientError::InvalidResponse(format!("Invalid JSON: {e}")))
    }

    /// Convert the response into a byte stream
    ///
    /// If the response is already buffered, this wraps it in a single-item stream.
    #[must_use]
    pub fn into_stream(self) -> BoxStream<'static, Result<Bytes, ClientError>> {
        match self.body {
            ResponseBody::Buffered(bytes) => {
                Box::pin(futures::stream::once(async move { Ok(bytes) }))
            }
            ResponseBody::Streaming(stream) => stream,
        }
    }

    /// Convert the response into an SSE event stream
    ///
    /// This is a convenience method for consuming Server-Sent Events.
    #[must_use]
    pub fn into_sse_stream(self) -> crate::sse::SseEventStream {
        crate::sse::SseEventStream::new(self.into_stream())
    }

    /// Check if response indicates a gateway error
    #[must_use]
    pub fn is_gateway_error(&self) -> bool {
        self.is_error() && self.error_source.is_gateway()
    }

    /// Check if response indicates an upstream error
    #[must_use]
    pub fn is_upstream_error(&self) -> bool {
        self.is_error() && self.error_source.is_upstream()
    }
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response")
            .field("status", &self.status)
            .field("headers", &self.headers)
            .field("body", &self.body)
            .field("error_source", &self.error_source)
            .finish()
    }
}
