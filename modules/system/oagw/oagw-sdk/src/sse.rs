//! Server-Sent Events (SSE) parsing
//!
//! Provides SSE event stream parsing for streaming responses from services
//! like OpenAI and Anthropic that use SSE for streaming completions.

use crate::error::ClientError;
use bytes::Bytes;
use futures::stream::BoxStream;
use futures::StreamExt;

/// SSE event stream parser
///
/// Parses Server-Sent Events from a byte stream according to the SSE specification.
///
/// # Examples
/// ```ignore
/// let response = client.execute("openai", request).await?;
/// let mut sse = response.into_sse_stream();
///
/// while let Some(event) = sse.next_event().await? {
///     if event.data.contains("[DONE]") {
///         break;
///     }
///     println!("Event: {}", event.data);
/// }
/// ```
pub struct SseEventStream {
    /// Underlying byte stream
    inner: BoxStream<'static, Result<Bytes, ClientError>>,

    /// Buffer for partial events
    buffer: Vec<u8>,

    /// Whether the stream has ended
    ended: bool,
}

impl SseEventStream {
    /// Create a new SSE event stream from a byte stream
    #[must_use]
    pub fn new(stream: BoxStream<'static, Result<Bytes, ClientError>>) -> Self {
        Self {
            inner: stream,
            buffer: Vec::new(),
            ended: false,
        }
    }

    /// Get the next SSE event from the stream
    ///
    /// Returns `Ok(None)` when the stream ends.
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - SSE parsing fails
    pub async fn next_event(&mut self) -> Result<Option<SseEvent>, ClientError> {
        if self.ended {
            return Ok(None);
        }

        loop {
            // Check if buffer contains a complete event (terminated by \n\n)
            if let Some(event) = self.parse_buffered_event()? {
                return Ok(Some(event));
            }

            // Read more data from stream
            match self.inner.next().await {
                Some(Ok(chunk)) => {
                    self.buffer.extend_from_slice(&chunk);
                }
                Some(Err(e)) => {
                    self.ended = true;
                    return Err(e);
                }
                None => {
                    self.ended = true;
                    // If buffer has remaining data, try to parse it as a final event
                    if !self.buffer.is_empty() {
                        return self.parse_buffered_event();
                    }
                    return Ok(None);
                }
            }
        }
    }

    /// Parse a complete event from the buffer
    ///
    /// Returns `Ok(None)` if no complete event is available yet.
    fn parse_buffered_event(&mut self) -> Result<Option<SseEvent>, ClientError> {
        // Find event terminator (\n\n or \r\n\r\n)
        let terminator_pos = self
            .buffer
            .windows(2)
            .position(|w| w == b"\n\n")
            .or_else(|| {
                self.buffer
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|pos| pos + 2)
            });

        if let Some(pos) = terminator_pos {
            // Extract event data
            let event_data = self.buffer.drain(..pos).collect::<Vec<u8>>();

            // Remove terminator
            if self.buffer.starts_with(b"\n\n") {
                self.buffer.drain(..2);
            } else if self.buffer.starts_with(b"\r\n\r\n") {
                self.buffer.drain(..4);
            }

            // Parse event
            let event = Self::parse_sse_event(&event_data)?;
            return Ok(Some(event));
        }

        Ok(None)
    }

    /// Parse SSE event from raw bytes
    fn parse_sse_event(data: &[u8]) -> Result<SseEvent, ClientError> {
        let mut id = None;
        let mut event = None;
        let mut event_data = Vec::new();
        let mut retry = None;

        // Parse lines
        for line in data.split(|&b| b == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with(b":") {
                continue;
            }

            // Parse field: value
            if let Some(colon_pos) = line.iter().position(|&b| b == b':') {
                let field = &line[..colon_pos];
                let mut value = &line[colon_pos + 1..];

                // Skip optional space after colon
                if value.starts_with(b" ") {
                    value = &value[1..];
                }

                match field {
                    b"id" => {
                        id = Some(
                            String::from_utf8(value.to_vec()).map_err(|e| {
                                ClientError::InvalidResponse(format!("Invalid UTF-8 in id: {e}"))
                            })?,
                        );
                    }
                    b"event" => {
                        event = Some(
                            String::from_utf8(value.to_vec()).map_err(|e| {
                                ClientError::InvalidResponse(format!("Invalid UTF-8 in event: {e}"))
                            })?,
                        );
                    }
                    b"data" => {
                        event_data.push(value.to_vec());
                    }
                    b"retry" => {
                        let retry_str = String::from_utf8(value.to_vec()).map_err(|e| {
                            ClientError::InvalidResponse(format!("Invalid UTF-8 in retry: {e}"))
                        })?;
                        retry = retry_str.parse::<u64>().ok();
                    }
                    _ => {
                        // Unknown field, ignore per SSE spec
                    }
                }
            }
        }

        // Join data lines with newlines
        let data_str = event_data
            .into_iter()
            .map(|bytes| {
                String::from_utf8(bytes)
                    .map_err(|e| ClientError::InvalidResponse(format!("Invalid UTF-8 in data: {e}")))
            })
            .collect::<Result<Vec<String>, ClientError>>()?
            .join("\n");

        Ok(SseEvent {
            id,
            event,
            data: data_str,
            retry,
        })
    }
}

/// SSE event
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// Event ID (from id field)
    pub id: Option<String>,

    /// Event type (from event field)
    pub event: Option<String>,

    /// Event data (from data field, multiple lines joined with \n)
    pub data: String,

    /// Retry delay in milliseconds (from retry field)
    pub retry: Option<u64>,
}

impl SseEvent {
    /// Parse JSON data from event
    ///
    /// # Errors
    /// Returns error if data is not valid JSON
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, ClientError> {
        serde_json::from_str(&self.data)
            .map_err(|e| ClientError::InvalidResponse(format!("Invalid JSON in SSE event: {e}")))
    }

    /// Check if event is a specific type
    #[must_use]
    pub fn is_event(&self, event_type: &str) -> bool {
        self.event.as_deref() == Some(event_type)
    }

    /// Check if event data contains a string
    #[must_use]
    pub fn contains(&self, s: &str) -> bool {
        self.data.contains(s)
    }
}
