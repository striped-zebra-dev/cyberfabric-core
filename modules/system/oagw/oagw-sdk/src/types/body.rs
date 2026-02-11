//! Request/response body types

use bytes::Bytes;
use futures::stream::BoxStream;
use std::io;

/// HTTP request/response body
///
/// Supports empty bodies, buffered bytes, and streaming bodies.
pub enum Body {
    /// No body (e.g., GET requests)
    Empty,

    /// Buffered body (entire content in memory)
    Bytes(Bytes),

    /// Streaming body (consumed incrementally)
    Stream(BoxStream<'static, Result<Bytes, io::Error>>),
}

impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Bytes(bytes) => f.debug_tuple("Bytes").field(&format!("{} bytes", bytes.len())).finish(),
            Self::Stream(_) => f.debug_tuple("Stream").field(&"<stream>").finish(),
        }
    }
}

impl Body {
    /// Create an empty body
    #[must_use]
    pub const fn empty() -> Self {
        Self::Empty
    }

    /// Create a body from bytes
    #[must_use]
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }

    /// Create a body from a byte vector
    #[must_use]
    pub fn from_vec(vec: Vec<u8>) -> Self {
        Self::Bytes(Bytes::from(vec))
    }

    /// Create a body from a static byte slice
    #[must_use]
    pub fn from_static(bytes: &'static [u8]) -> Self {
        Self::Bytes(Bytes::from_static(bytes))
    }

    /// Create a body from a stream
    #[must_use]
    pub fn from_stream(stream: BoxStream<'static, Result<Bytes, io::Error>>) -> Self {
        Self::Stream(stream)
    }

    /// Check if body is empty
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }

    /// Check if body is buffered
    #[must_use]
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Bytes(_))
    }

    /// Check if body is streaming
    #[must_use]
    pub const fn is_streaming(&self) -> bool {
        matches!(self, Self::Stream(_))
    }
}

impl Default for Body {
    fn default() -> Self {
        Self::Empty
    }
}

impl From<Bytes> for Body {
    fn from(bytes: Bytes) -> Self {
        Self::Bytes(bytes)
    }
}

impl From<Vec<u8>> for Body {
    fn from(vec: Vec<u8>) -> Self {
        Self::Bytes(Bytes::from(vec))
    }
}

impl From<&'static [u8]> for Body {
    fn from(bytes: &'static [u8]) -> Self {
        Self::Bytes(Bytes::from_static(bytes))
    }
}

impl From<String> for Body {
    fn from(s: String) -> Self {
        Self::Bytes(Bytes::from(s))
    }
}

impl From<&'static str> for Body {
    fn from(s: &'static str) -> Self {
        Self::Bytes(Bytes::from_static(s.as_bytes()))
    }
}
