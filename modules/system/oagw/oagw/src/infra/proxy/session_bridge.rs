use std::io::Write as _;

use anyhow::Context;
use bytes::{Buf, Bytes, BytesMut};
use futures_util::stream::unfold;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode};
use oagw_sdk::body::{BodyStream, BoxError};
use tokio::io::{AsyncRead, AsyncReadExt};

// ---------------------------------------------------------------------------
// Request serialization
// ---------------------------------------------------------------------------

/// Extract path and query from a full URL.
/// e.g. `"https://api.example.com/v1/chat?k=v"` → `"/v1/chat?k=v"`
fn url_path_and_query(url: &str) -> &str {
    if let Some(scheme_end) = url.find("://") {
        let after_scheme = &url[scheme_end + 3..];
        if let Some(path_start) = after_scheme.find('/') {
            return &after_scheme[path_start..];
        }
        return "/";
    }
    url
}

/// Serialize an HTTP/1.1 request with a buffered body to wire format.
///
/// Produces: request-line, headers, Content-Length (if body non-empty),
/// blank line, body bytes.
pub(crate) fn serialize_request(
    method: &Method,
    url: &str,
    headers: &HeaderMap,
    body: &Bytes,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(512 + body.len());
    let _ = write!(buf, "{} {} HTTP/1.1\r\n", method, url_path_and_query(url));
    for (name, value) in headers {
        buf.extend_from_slice(name.as_str().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(value.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
    // Always include Content-Length so Pingora knows the body boundary.
    let _ = write!(buf, "Content-Length: {}\r\n", body.len());
    // Single-shot bridge — no keep-alive on the in-memory session.
    buf.extend_from_slice(b"Connection: close\r\n");
    buf.extend_from_slice(b"\r\n");
    buf.extend_from_slice(body);
    buf
}

/// Serialize HTTP/1.1 request headers only (no Content-Length, no body).
///
/// Used for the `Body::Stream` (WebSocket) bridge path where the body is
/// forwarded as raw stream chunks after the headers.
pub(crate) fn serialize_request_headers(
    method: &Method,
    url: &str,
    headers: &HeaderMap,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(512);
    let _ = write!(buf, "{} {} HTTP/1.1\r\n", method, url_path_and_query(url));
    for (name, value) in headers {
        buf.extend_from_slice(name.as_str().as_bytes());
        buf.extend_from_slice(b": ");
        buf.extend_from_slice(value.as_bytes());
        buf.extend_from_slice(b"\r\n");
    }
    // Single-shot bridge — no keep-alive on the in-memory session.
    buf.extend_from_slice(b"Connection: close\r\n");
    buf.extend_from_slice(b"\r\n");
    buf
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Read an HTTP/1.1 response from the client side of a DuplexStream.
///
/// Parses the status line and headers via `httparse`, then returns a
/// streaming body whose framing strategy depends on the response:
///
/// - **101 Switching Protocols** → raw unbounded byte stream (WebSocket)
/// - **Content-Length** → exactly N bytes
/// - **Transfer-Encoding: chunked** → decoded chunks
/// - **Otherwise** → read until EOF
pub(crate) async fn parse_response_stream(
    mut io: impl AsyncRead + Unpin + Send + 'static,
) -> anyhow::Result<(StatusCode, HeaderMap, BodyStream)> {
    // Phase 1: accumulate bytes until httparse can parse a complete header.
    let mut buf = BytesMut::with_capacity(4096);
    let (status, headers, body_offset) = loop {
        let mut tmp = [0u8; 4096];
        let n = io
            .read(&mut tmp)
            .await
            .context("failed to read response from proxy")?;
        if n == 0 {
            anyhow::bail!("proxy closed connection before sending response headers");
        }
        buf.extend_from_slice(&tmp[..n]);

        let mut parsed_headers = [httparse::EMPTY_HEADER; 64];
        let mut resp = httparse::Response::new(&mut parsed_headers);
        match resp.parse(&buf)? {
            httparse::Status::Complete(offset) => {
                let status = StatusCode::from_u16(resp.code.unwrap_or(502))?;
                let mut headers = HeaderMap::new();
                for h in resp.headers.iter() {
                    if let (Ok(name), Ok(value)) = (
                        HeaderName::from_bytes(h.name.as_bytes()),
                        HeaderValue::from_bytes(h.value),
                    ) {
                        headers.append(name, value);
                    }
                }
                break (status, headers, offset);
            }
            httparse::Status::Partial => continue,
        }
    };

    // Leftover body bytes that were read together with the headers.
    let _ = buf.split_to(body_offset);
    let remaining = buf.freeze();

    // Phase 2: select body-reading strategy.
    let body_stream = if status == StatusCode::SWITCHING_PROTOCOLS {
        raw_body_stream(remaining, io)
    } else if let Some(len) = content_length_value(&headers) {
        content_length_body_stream(remaining, io, len)
    } else if is_chunked_encoding(&headers) {
        chunked_body_stream(remaining, io)
    } else {
        raw_body_stream(remaining, io)
    };

    Ok((status, headers, body_stream))
}

fn content_length_value(headers: &HeaderMap) -> Option<usize> {
    headers
        .get(http::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn is_chunked_encoding(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::TRANSFER_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"))
}

// ---------------------------------------------------------------------------
// Body stream builders
// ---------------------------------------------------------------------------

/// Read raw bytes until EOF (used for 101 Upgrade and connection-close).
fn raw_body_stream<R: AsyncRead + Unpin + Send + 'static>(initial: Bytes, io: R) -> BodyStream {
    struct State<R> {
        io: R,
        initial: Option<Bytes>,
    }

    Box::pin(unfold(
        State {
            io,
            initial: if initial.is_empty() {
                None
            } else {
                Some(initial)
            },
        },
        |mut state| async move {
            if let Some(initial) = state.initial.take() {
                return Some((Ok(initial), state));
            }
            let mut buf = vec![0u8; 8192];
            match state.io.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    Some((Ok(Bytes::from(buf)), state))
                }
                Err(e) => Some((Err(Box::new(e) as BoxError), state)),
            }
        },
    ))
}

/// Read exactly `total` body bytes (Content-Length delimited).
fn content_length_body_stream<R: AsyncRead + Unpin + Send + 'static>(
    initial: Bytes,
    io: R,
    total: usize,
) -> BodyStream {
    struct State<R> {
        io: R,
        remaining: usize,
        initial: Option<Bytes>,
    }

    Box::pin(unfold(
        State {
            io,
            remaining: total,
            initial: if initial.is_empty() {
                None
            } else {
                Some(initial)
            },
        },
        |mut state| async move {
            if state.remaining == 0 {
                return None;
            }
            if let Some(initial) = state.initial.take() {
                let to_take = initial.len().min(state.remaining);
                state.remaining -= to_take;
                return Some((Ok(initial.slice(..to_take)), state));
            }
            let to_read = state.remaining.min(8192);
            let mut buf = vec![0u8; to_read];
            match state.io.read(&mut buf).await {
                Ok(0) => None,
                Ok(n) => {
                    buf.truncate(n);
                    state.remaining -= n;
                    Some((Ok(Bytes::from(buf)), state))
                }
                Err(e) => Some((Err(Box::new(e) as BoxError), state)),
            }
        },
    ))
}

/// Decode chunked transfer encoding into plain body chunks.
fn chunked_body_stream<R: AsyncRead + Unpin + Send + 'static>(initial: Bytes, io: R) -> BodyStream {
    struct State<R> {
        io: R,
        buf: BytesMut,
    }

    Box::pin(unfold(
        State {
            io,
            buf: BytesMut::from(initial.as_ref()),
        },
        |mut state| async move {
            loop {
                // Look for the chunk-size line terminator.
                if let Some(pos) = find_crlf(&state.buf) {
                    let size_hex = std::str::from_utf8(&state.buf[..pos])
                        .ok()?
                        .split(';')
                        .next()?
                        .trim();
                    let chunk_size = usize::from_str_radix(size_hex, 16).ok()?;

                    // Advance past the size line.
                    state.buf.advance(pos + 2);

                    if chunk_size == 0 {
                        return None;
                    }

                    // Ensure we have chunk_size + trailing CRLF bytes.
                    while state.buf.len() < chunk_size + 2 {
                        if fill_buf(&mut state.io, &mut state.buf).await.is_err() {
                            return None;
                        }
                    }

                    let chunk = state.buf.split_to(chunk_size).freeze();
                    state.buf.advance(2); // trailing \r\n
                    return Some((Ok(chunk), state));
                }

                // Need more data from the stream.
                if fill_buf(&mut state.io, &mut state.buf).await.is_err() {
                    return None;
                }
            }
        },
    ))
}

fn find_crlf(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\r\n")
}

async fn fill_buf<R: AsyncRead + Unpin>(
    io: &mut R,
    buf: &mut BytesMut,
) -> Result<usize, std::io::Error> {
    let mut tmp = [0u8; 8192];
    let n = io.read(&mut tmp).await?;
    if n == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "unexpected EOF in chunked body",
        ));
    }
    buf.extend_from_slice(&tmp[..n]);
    Ok(n)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio::io::DuplexStream;
    // Use fully-qualified `AsyncWriteExt` to avoid ambiguity with
    // pingora_core::protocols::Shutdown (also implemented for DuplexStream).
    use tokio::io::AsyncWriteExt as _;

    /// Shutdown the write side of a DuplexStream (disambiguated).
    async fn shut(w: &mut DuplexStream) {
        tokio::io::AsyncWriteExt::shutdown(w).await.unwrap();
    }

    // -- serialize_request tests (task 2.4) --

    #[test]
    fn serialize_request_line_format() {
        let headers = HeaderMap::new();
        let body = Bytes::new();
        let wire = serialize_request(&Method::GET, "https://example.com/v1/chat", &headers, &body);
        let text = String::from_utf8_lossy(&wire);
        assert!(text.starts_with("GET /v1/chat HTTP/1.1\r\n"));
    }

    #[test]
    fn serialize_request_includes_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("example.com"));
        headers.insert("x-api-key", HeaderValue::from_static("secret"));
        let body = Bytes::new();
        let wire = serialize_request(&Method::POST, "https://example.com/api", &headers, &body);
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("host: example.com\r\n"));
        assert!(text.contains("x-api-key: secret\r\n"));
    }

    #[test]
    fn serialize_request_content_length_with_body() {
        let headers = HeaderMap::new();
        let body = Bytes::from_static(b"hello world");
        let wire = serialize_request(&Method::POST, "https://example.com/api", &headers, &body);
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(wire.ends_with(b"hello world"));
    }

    #[test]
    fn serialize_request_content_length_zero_for_empty_body() {
        let headers = HeaderMap::new();
        let body = Bytes::new();
        let wire = serialize_request(&Method::GET, "https://example.com/api", &headers, &body);
        let text = String::from_utf8_lossy(&wire);
        assert!(text.contains("Content-Length: 0\r\n"));
        assert!(text.contains("Connection: close\r\n"));
    }

    #[test]
    fn serialize_request_url_without_scheme() {
        let wire = serialize_request(
            &Method::GET,
            "/plain/path",
            &HeaderMap::new(),
            &Bytes::new(),
        );
        let text = String::from_utf8_lossy(&wire);
        assert!(text.starts_with("GET /plain/path HTTP/1.1\r\n"));
    }

    // -- serialize_request_headers tests (task 2.5) --

    #[test]
    fn serialize_headers_only_no_content_length() {
        let mut headers = HeaderMap::new();
        headers.insert("upgrade", HeaderValue::from_static("websocket"));
        let wire = serialize_request_headers(&Method::GET, "wss://example.com/ws", &headers);
        let text = String::from_utf8_lossy(&wire);
        assert!(!text.contains("Content-Length"));
        assert!(text.contains("upgrade: websocket\r\n"));
        assert!(text.contains("Connection: close\r\n"));
        assert!(wire.ends_with(b"\r\n\r\n"));
    }

    #[test]
    fn serialize_headers_only_no_body_bytes() {
        let wire =
            serialize_request_headers(&Method::GET, "https://example.com/api", &HeaderMap::new());
        // After the final \r\n\r\n there must be nothing.
        let text = String::from_utf8_lossy(&wire);
        assert!(text.ends_with("\r\n\r\n"));
        let parts: Vec<&str> = text.splitn(2, "\r\n\r\n").collect();
        assert_eq!(parts.len(), 2);
        assert!(parts[1].is_empty());
    }

    // -- parse_response_stream tests (task 2.6) --

    #[tokio::test]
    async fn parse_response_content_length() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        let body = b"hello world";
        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        tokio::spawn(async move {
            writer.write_all(resp.as_bytes()).await.unwrap();
            writer.write_all(body).await.unwrap();
            shut(&mut writer).await;
        });

        let (status, headers, body_stream) = parse_response_stream(reader).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            headers.get("content-length").unwrap().to_str().unwrap(),
            "11"
        );

        let chunks: Vec<Bytes> = body_stream.map(|r| r.unwrap()).collect().await;
        let all: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(all, b"hello world");
    }

    #[tokio::test]
    async fn parse_response_chunked() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            writer
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await
                .unwrap();
            writer.write_all(b"5\r\nhello\r\n").await.unwrap();
            writer.write_all(b"6\r\n world\r\n").await.unwrap();
            writer.write_all(b"0\r\n\r\n").await.unwrap();
            shut(&mut writer).await;
        });

        let (status, _headers, body_stream) = parse_response_stream(reader).await.unwrap();
        assert_eq!(status, StatusCode::OK);

        let chunks: Vec<Bytes> = body_stream.map(|r| r.unwrap()).collect().await;
        let all: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(all, b"hello world");
    }

    #[tokio::test]
    async fn parse_response_101_upgrade() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        tokio::spawn(async move {
            writer
                .write_all(
                    b"HTTP/1.1 101 Switching Protocols\r\n\
                      Upgrade: websocket\r\n\
                      Connection: Upgrade\r\n\r\n\
                      raw ws frames here",
                )
                .await
                .unwrap();
            shut(&mut writer).await;
        });

        let (status, headers, body_stream) = parse_response_stream(reader).await.unwrap();
        assert_eq!(status, StatusCode::SWITCHING_PROTOCOLS);
        assert_eq!(
            headers.get("upgrade").unwrap().to_str().unwrap(),
            "websocket"
        );

        let chunks: Vec<Bytes> = body_stream.map(|r| r.unwrap()).collect().await;
        let all: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(all, b"raw ws frames here");
    }

    #[tokio::test]
    async fn parse_response_error_502() {
        let (mut writer, reader) = tokio::io::duplex(4096);
        let body = br#"{"status":502,"detail":"upstream error"}"#;
        let resp = format!(
            "HTTP/1.1 502 Bad Gateway\r\nContent-Length: {}\r\n\r\n",
            body.len()
        );
        tokio::spawn(async move {
            writer.write_all(resp.as_bytes()).await.unwrap();
            writer.write_all(body).await.unwrap();
            shut(&mut writer).await;
        });

        let (status, _headers, body_stream) = parse_response_stream(reader).await.unwrap();
        assert_eq!(status, StatusCode::BAD_GATEWAY);

        let chunks: Vec<Bytes> = body_stream.map(|r| r.unwrap()).collect().await;
        let all: Vec<u8> = chunks.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(all, body.as_slice());
    }
}
