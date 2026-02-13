use std::fmt;
use std::pin::Pin;

use bytes::{Bytes, BytesMut};
use futures::stream::{Stream, StreamExt};

use crate::error::ClientError;

// Type aliases for convenience
type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

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
    pub fn from_json<T: serde::Serialize>(value: &T) -> Result<Self, serde_json::Error> {
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
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn test_body_empty() {
        let body = Body::empty();
        assert!(matches!(body, Body::Empty));
    }

    #[test]
    fn test_body_from_bytes() {
        let bytes = Bytes::from("test data");
        let body = Body::from_bytes(bytes.clone());
        match body {
            Body::Bytes(b) => assert_eq!(b, bytes),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_body_from_json() {
        let value = serde_json::json!({"key": "value", "num": 42});
        let body = Body::from_json(&value).unwrap();
        match body {
            Body::Bytes(b) => {
                let parsed: serde_json::Value = serde_json::from_slice(&b).unwrap();
                assert_eq!(parsed, value);
            }
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_body_from_string() {
        let body = Body::from("test");
        match body {
            Body::Bytes(b) => assert_eq!(b, "test"),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_body_from_string_owned() {
        let body = Body::from("test".to_string());
        match body {
            Body::Bytes(b) => assert_eq!(b, "test"),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_body_from_vec() {
        let vec = vec![1, 2, 3, 4];
        let body = Body::from(vec.clone());
        match body {
            Body::Bytes(b) => assert_eq!(b, Bytes::from(vec)),
            _ => panic!("Expected Bytes variant"),
        }
    }

    #[test]
    fn test_body_from_unit() {
        let body = Body::from(());
        assert!(matches!(body, Body::Empty));
    }

    #[tokio::test]
    async fn test_body_into_bytes_empty() {
        let body = Body::empty();
        let bytes = body.into_bytes().await.unwrap();
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn test_body_into_bytes_from_bytes() {
        let body = Body::from("test");
        let bytes = body.into_bytes().await.unwrap();
        assert_eq!(bytes, "test");
    }

    #[tokio::test]
    async fn test_body_into_bytes_from_stream() {
        let chunks = vec![
            Ok(Bytes::from("hello ")),
            Ok(Bytes::from("world")),
        ];
        let stream = stream::iter(chunks);
        let body = Body::from_stream(stream);

        let bytes = body.into_bytes().await.unwrap();
        assert_eq!(bytes, "hello world");
    }

    #[tokio::test]
    async fn test_body_into_bytes_stream_error() {
        let chunks = vec![
            Ok(Bytes::from("hello")),
            Err(std::io::Error::new(std::io::ErrorKind::Other, "test error")),
        ];
        let stream = stream::iter(chunks);
        let body = Body::from_stream(stream);

        let result = body.into_bytes().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_body_debug_empty() {
        let body = Body::empty();
        let debug = format!("{:?}", body);
        assert_eq!(debug, "Body::Empty");
    }

    #[test]
    fn test_body_debug_bytes() {
        let body = Body::from("test data");
        let debug = format!("{:?}", body);
        assert_eq!(debug, "Body::Bytes(9 bytes)");
    }

    #[test]
    fn test_body_debug_stream() {
        let stream = stream::iter(vec![Ok(Bytes::from("test"))]);
        let body = Body::from_stream(stream);
        let debug = format!("{:?}", body);
        assert_eq!(debug, "Body::Stream");
    }
}