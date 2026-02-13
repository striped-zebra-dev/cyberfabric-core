use std::pin::Pin;

use bytes::{Bytes};
use futures::stream::{Stream, StreamExt};

use crate::error::ClientError;

// Type aliases for convenience
type BoxStream<'a, T> = Pin<Box<dyn Stream<Item = T> + Send + 'a>>;

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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[test]
    fn test_sse_event_parsing() {
        let data = b"id: 123\nevent: message\ndata: hello\ndata: world\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.id, Some("123".to_string()));
        assert_eq!(event.event, Some("message".to_string()));
        assert_eq!(event.data, "hello\nworld");
    }

    #[test]
    fn test_sse_event_parsing_minimal() {
        let data = b"data: test\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.id, None);
        assert_eq!(event.event, None);
        assert_eq!(event.data, "test");
        assert_eq!(event.retry, None);
    }

    #[test]
    fn test_sse_event_parsing_with_retry() {
        let data = b"retry: 3000\ndata: reconnect\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.retry, Some(3000));
        assert_eq!(event.data, "reconnect");
    }

    #[test]
    fn test_sse_event_parsing_empty_data() {
        let data = b"event: ping\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.event, Some("ping".to_string()));
        assert_eq!(event.data, "");
    }

    #[test]
    fn test_sse_event_parsing_multiline_data() {
        let data = b"data: line1\ndata: line2\ndata: line3\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.data, "line1\nline2\nline3");
    }

    #[test]
    fn test_sse_event_parsing_with_space_after_colon() {
        let data = b"id:123\nevent:message\ndata:no space\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.id, Some("123".to_string()));
        assert_eq!(event.event, Some("message".to_string()));
        assert_eq!(event.data, "no space");
    }

    #[test]
    fn test_sse_event_parsing_unknown_fields() {
        let data = b"id: 1\ndata: test\nunknown: field\n\n";
        let event = SseEventStream::parse_sse_event(data).unwrap();

        assert_eq!(event.id, Some("1".to_string()));
        assert_eq!(event.data, "test");
    }

    #[tokio::test]
    async fn test_sse_stream_single_event() {
        let chunks = vec![Ok(Bytes::from("data: hello\n\n"))];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let event = sse_stream.next_event().await.unwrap();
        assert!(event.is_some());
        let event = event.unwrap();
        assert_eq!(event.data, "hello");
    }

    #[tokio::test]
    async fn test_sse_stream_multiple_events() {
        let chunks = vec![
            Ok(Bytes::from("data: event1\n\ndata: event2\n\n")),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let event1 = sse_stream.next_event().await.unwrap().unwrap();
        assert_eq!(event1.data, "event1");

        let event2 = sse_stream.next_event().await.unwrap().unwrap();
        assert_eq!(event2.data, "event2");

        let no_more = sse_stream.next_event().await.unwrap();
        assert!(no_more.is_none());
    }

    #[tokio::test]
    async fn test_sse_stream_fragmented_data() {
        let chunks = vec![
            Ok(Bytes::from("data: hel")),
            Ok(Bytes::from("lo\n\n")),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let event = sse_stream.next_event().await.unwrap().unwrap();
        assert_eq!(event.data, "hello");
    }

    #[tokio::test]
    async fn test_sse_stream_empty() {
        let chunks: Vec<Result<Bytes, ClientError>> = vec![];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let event = sse_stream.next_event().await.unwrap();
        assert!(event.is_none());
    }

    #[tokio::test]
    async fn test_sse_stream_error() {
        let chunks = vec![
            Ok(Bytes::from("data: test")),
            Err(ClientError::ConnectionClosed),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let result = sse_stream.next_event().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_sse_stream_partial_data_at_end() {
        let chunks = vec![
            Ok(Bytes::from("data: complete\n\ndata: incomplete")),
        ];
        let stream = Box::pin(stream::iter(chunks));
        let mut sse_stream = SseEventStream::new(stream);

        let event1 = sse_stream.next_event().await.unwrap().unwrap();
        assert_eq!(event1.data, "complete");

        // Partial data without double newline at stream end won't be parsed
        let event2 = sse_stream.next_event().await.unwrap();
        assert!(event2.is_none());
    }

    #[test]
    fn test_sse_event_clone() {
        let event = SseEvent {
            id: Some("123".to_string()),
            event: Some("message".to_string()),
            data: "test data".to_string(),
            retry: Some(5000),
        };

        let cloned = event.clone();
        assert_eq!(event.id, cloned.id);
        assert_eq!(event.event, cloned.event);
        assert_eq!(event.data, cloned.data);
        assert_eq!(event.retry, cloned.retry);
    }

    #[test]
    fn test_sse_event_debug() {
        let event = SseEvent {
            id: Some("1".to_string()),
            event: None,
            data: "test".to_string(),
            retry: None,
        };

        let debug = format!("{:?}", event);
        assert!(debug.contains("SseEvent"));
        assert!(debug.contains("test"));
    }
}