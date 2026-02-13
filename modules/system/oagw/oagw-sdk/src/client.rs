use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::sync::mpsc;

use crate::request::Request;
use crate::response::Response;
use crate::error::ClientError;
use crate::service::DataPlaneService;

// Re-export types for convenience
pub use crate::body::Body;
pub use crate::request::{Request as HttpRequest, RequestBuilder};
pub use crate::response::Response as HttpResponse;

// ===========================================================================
// WebSocket Types (Phase 1)
// ===========================================================================

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

    #[tokio::test]
    async fn test_websocket_conn_send() {
        let (tx, _rx) = mpsc::channel(10);
        let (_msg_tx, msg_rx) = mpsc::channel(10);
        let mut conn = WebSocketConn::new(tx, msg_rx);

        let msg = WsMessage::Text("hello".to_string());
        let result = conn.send(msg.clone()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_websocket_conn_recv() {
        let (_tx, _rx) = mpsc::channel(10);
        let (msg_tx, msg_rx) = mpsc::channel(10);
        let mut conn = WebSocketConn::new(_tx, msg_rx);

        // Send a message to the receiver
        let msg = WsMessage::Text("hello".to_string());
        msg_tx.send(Ok(msg.clone())).await.unwrap();

        let received = conn.recv().await.unwrap();
        assert!(received.is_some());
    }

    #[tokio::test]
    async fn test_websocket_conn_close() {
        let (tx, _rx) = mpsc::channel(10);
        let (_msg_tx, msg_rx) = mpsc::channel(10);
        let conn = WebSocketConn::new(tx, msg_rx);

        let result = conn.close().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_websocket_conn_recv_error() {
        let (_tx, _rx) = mpsc::channel(10);
        let (msg_tx, msg_rx) = mpsc::channel(10);
        let mut conn = WebSocketConn::new(_tx, msg_rx);

        // Send an error
        msg_tx.send(Err(ClientError::ConnectionClosed)).await.unwrap();

        let result = conn.recv().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_websocket_conn_send_after_close() {
        let (tx, _rx) = mpsc::channel(1);
        let (_msg_tx, msg_rx) = mpsc::channel(1);

        // Drop receiver to simulate closed connection
        drop(_rx);

        let mut conn = WebSocketConn::new(tx, msg_rx);
        let msg = WsMessage::Text("test".to_string());

        let result = conn.send(msg).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ClientError::ConnectionClosed));
    }

    #[test]
    fn test_ws_message_variants() {
        let text = WsMessage::Text("hello".to_string());
        assert!(matches!(text, WsMessage::Text(_)));

        let binary = WsMessage::Binary(Bytes::from("data"));
        assert!(matches!(binary, WsMessage::Binary(_)));

        let ping = WsMessage::Ping(vec![1, 2, 3]);
        assert!(matches!(ping, WsMessage::Ping(_)));

        let pong = WsMessage::Pong(vec![4, 5, 6]);
        assert!(matches!(pong, WsMessage::Pong(_)));

        let close = WsMessage::Close(Some(CloseFrame {
            code: 1000,
            reason: "Normal closure".to_string(),
        }));
        assert!(matches!(close, WsMessage::Close(_)));
    }

    #[test]
    fn test_close_frame() {
        let frame = CloseFrame {
            code: 1001,
            reason: "Going away".to_string(),
        };
        assert_eq!(frame.code, 1001);
        assert_eq!(frame.reason, "Going away");
    }

    #[test]
    fn test_oagw_client_config_remote_proxy() {
        let config = OagwClientConfig::remote_proxy(
            "http://localhost:8080".to_string(),
            "test-token".to_string(),
        );

        assert_eq!(config.default_timeout, Duration::from_secs(30));
        match config.mode {
            ClientMode::RemoteProxy { base_url, auth_token, timeout } => {
                assert_eq!(base_url, "http://localhost:8080");
                assert_eq!(auth_token, "test-token");
                assert_eq!(timeout, Duration::from_secs(30));
            }
            _ => panic!("Expected RemoteProxy mode"),
        }
    }
}
