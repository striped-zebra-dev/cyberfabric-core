//! Tests for SSE (Server-Sent Events) parsing

use bytes::Bytes;
use futures::stream;
use oagw_sdk::sse::SseEventStream;

#[tokio::test]
async fn test_parse_simple_sse_event() {
    let data = b"data: hello world\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "hello world");
    assert_eq!(event.id, None);
    assert_eq!(event.event, None);
    assert_eq!(event.retry, None);
}

#[tokio::test]
async fn test_parse_sse_event_with_id() {
    let data = b"id: 123\ndata: test data\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "test data");
    assert_eq!(event.id, Some("123".to_string()));
}

#[tokio::test]
async fn test_parse_sse_event_with_event_type() {
    let data = b"event: message\ndata: test\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "test");
    assert_eq!(event.event, Some("message".to_string()));
    assert!(event.is_event("message"));
    assert!(!event.is_event("other"));
}

#[tokio::test]
async fn test_parse_sse_event_with_retry() {
    let data = b"retry: 5000\ndata: test\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "test");
    assert_eq!(event.retry, Some(5000));
}

#[tokio::test]
async fn test_parse_sse_event_with_all_fields() {
    let data = b"id: 42\nevent: update\ndata: hello\nretry: 3000\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.id, Some("42".to_string()));
    assert_eq!(event.event, Some("update".to_string()));
    assert_eq!(event.data, "hello");
    assert_eq!(event.retry, Some(3000));
}

#[tokio::test]
async fn test_parse_multiline_data() {
    let data = b"data: line 1\ndata: line 2\ndata: line 3\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "line 1\nline 2\nline 3");
}

#[tokio::test]
async fn test_parse_multiple_events() {
    let data = b"data: event 1\n\ndata: event 2\n\ndata: event 3\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event1 = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event1.data, "event 1");

    let event2 = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event2.data, "event 2");

    let event3 = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event3.data, "event 3");

    let no_more = sse.next_event().await.unwrap();
    assert!(no_more.is_none());
}

#[tokio::test]
async fn test_parse_openai_streaming_format() {
    // Simulate OpenAI streaming format
    let data = b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n\
                 data: [DONE]\n\n";

    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    // First event
    let event1 = sse.next_event().await.unwrap().unwrap();
    assert!(event1.data.contains("Hello"));
    let json: serde_json::Value = event1.json().unwrap();
    assert_eq!(json["choices"][0]["delta"]["content"], "Hello");

    // Second event
    let event2 = sse.next_event().await.unwrap().unwrap();
    assert!(event2.data.contains(" world"));

    // Done event
    let event3 = sse.next_event().await.unwrap().unwrap();
    assert!(event3.contains("[DONE]"));
}

#[tokio::test]
async fn test_parse_with_crlf_terminators() {
    let data = b"data: test\r\n\r\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "test");
}

#[tokio::test]
async fn test_parse_with_comments() {
    let data = b": this is a comment\ndata: actual data\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "actual data");
}

#[tokio::test]
async fn test_parse_empty_data() {
    let data = b"data: \n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "");
}

#[tokio::test]
async fn test_parse_chunked_stream() {
    // Simulate data arriving in multiple chunks
    let chunk1 = Bytes::from_static(b"data: hel");
    let chunk2 = Bytes::from_static(b"lo world\n\n");

    let stream = stream::iter(vec![Ok(chunk1), Ok(chunk2)]);
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "hello world");
}

#[tokio::test]
async fn test_parse_event_split_across_chunks() {
    // Event split across multiple chunks
    let chunks = vec![
        Bytes::from_static(b"id: 1"),
        Bytes::from_static(b"23\n"),
        Bytes::from_static(b"data: te"),
        Bytes::from_static(b"st\n\n"),
    ];

    let stream = stream::iter(chunks.into_iter().map(Ok));
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.id, Some("123".to_string()));
    assert_eq!(event.data, "test");
}

#[tokio::test]
async fn test_stream_end() {
    let data = b"data: last event\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    // Get the last event
    let event = sse.next_event().await.unwrap().unwrap();
    assert_eq!(event.data, "last event");

    // Stream should end
    let end = sse.next_event().await.unwrap();
    assert!(end.is_none());

    // Subsequent calls should also return None
    let still_end = sse.next_event().await.unwrap();
    assert!(still_end.is_none());
}

#[tokio::test]
async fn test_json_parsing() {
    let data = b"data: {\"key\": \"value\", \"number\": 42}\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    let json: serde_json::Value = event.json().unwrap();
    assert_eq!(json["key"], "value");
    assert_eq!(json["number"], 42);
}

#[tokio::test]
async fn test_invalid_json_error() {
    let data = b"data: {invalid json}\n\n";
    let stream = stream::once(async { Ok(Bytes::from_static(data)) });
    let mut sse = SseEventStream::new(Box::pin(stream));

    let event = sse.next_event().await.unwrap().unwrap();
    let result: Result<serde_json::Value, _> = event.json();
    assert!(result.is_err());
}
