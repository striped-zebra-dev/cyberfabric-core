//! Error types for OAGW SDK

use bytes::Bytes;
use http::StatusCode;
use std::io;

/// Error type for OAGW client operations
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Request build error
    ///
    /// Returned when request construction fails (missing required fields,
    /// invalid JSON serialization, etc.)
    #[error("Request build error: {0}")]
    BuildError(String),

    /// Connection error
    ///
    /// Returned when network connection to OAGW fails
    #[error("Connection error: {0}")]
    Connection(String),

    /// Request timeout
    ///
    /// Returned when request exceeds configured timeout
    #[error("Timeout: {0}")]
    Timeout(String),

    /// TLS/SSL error
    ///
    /// Returned when TLS handshake or certificate validation fails
    #[error("TLS error: {0}")]
    Tls(String),

    /// Protocol error
    ///
    /// Returned when HTTP protocol violations occur
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Connection closed unexpectedly
    ///
    /// Returned when connection is closed before response is complete
    #[error("Connection closed")]
    ConnectionClosed,

    /// Invalid response
    ///
    /// Returned when response cannot be parsed (invalid JSON, UTF-8, etc.)
    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    /// I/O error
    ///
    /// Returned when underlying I/O operations fail
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// HTTP error response
    ///
    /// Returned when server responds with an error status code (4xx, 5xx)
    #[error("HTTP error: {status}")]
    Http {
        /// HTTP status code
        status: StatusCode,

        /// Response body (may contain error details)
        body: Bytes,
    },

    /// Configuration error
    ///
    /// Returned when client configuration is invalid
    #[error("Configuration error: {0}")]
    Config(String),

    /// Reqwest error (from RemoteProxyClient)
    ///
    /// Returned when underlying reqwest client fails
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

impl ClientError {
    /// Check if error is a connection error
    #[must_use]
    pub const fn is_connection(&self) -> bool {
        matches!(self, Self::Connection(_) | Self::ConnectionClosed)
    }

    /// Check if error is a timeout
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }

    /// Check if error is a TLS error
    #[must_use]
    pub const fn is_tls(&self) -> bool {
        matches!(self, Self::Tls(_))
    }

    /// Check if error is an HTTP error
    #[must_use]
    pub const fn is_http(&self) -> bool {
        matches!(self, Self::Http { .. })
    }

    /// Get HTTP status code if this is an HTTP error
    #[must_use]
    pub const fn http_status(&self) -> Option<StatusCode> {
        match self {
            Self::Http { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Check if error is retryable
    ///
    /// Returns true for transient errors like connection failures and timeouts,
    /// but false for errors like invalid requests that won't succeed on retry.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Connection(_)
            | Self::Timeout(_)
            | Self::ConnectionClosed
            | Self::Io(_) => true,
            Self::Http { status, .. } => {
                // Retry on 5xx server errors and 429 rate limit
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            _ => false,
        }
    }
}

/// Result type alias for OAGW client operations
pub type Result<T> = std::result::Result<T, ClientError>;
