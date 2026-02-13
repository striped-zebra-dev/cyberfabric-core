use http::StatusCode;
use bytes::Bytes;


/// Gateway-originated error with all information needed to produce a Problem Details response.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServiceGatewayError {
    #[error("{detail}")]
    ValidationError { detail: String, instance: String },

    #[error("target host header required for multi-endpoint upstream")]
    MissingTargetHost { instance: String },

    #[error("invalid target host header format")]
    InvalidTargetHost { instance: String },

    #[error("{detail}")]
    UnknownTargetHost { detail: String, instance: String },

    #[error("{detail}")]
    AuthenticationFailed { detail: String, instance: String },

    #[error("no matching route found")]
    RouteNotFound { instance: String },

    #[error("{detail}")]
    PayloadTooLarge { detail: String, instance: String },

    #[error("{detail}")]
    RateLimitExceeded {
        detail: String,
        instance: String,
        retry_after_secs: Option<u64>,
    },

    #[error("{detail}")]
    SecretNotFound { detail: String, instance: String },

    #[error("{detail}")]
    DownstreamError { detail: String, instance: String },

    #[error("{detail}")]
    ProtocolError { detail: String, instance: String },

    #[error("{detail}")]
    UpstreamDisabled { detail: String, instance: String },

    #[error("{detail}")]
    ConnectionTimeout { detail: String, instance: String },

    #[error("{detail}")]
    RequestTimeout { detail: String, instance: String },
}


/// Client errors
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("Request build error: {0}")]
    BuildError(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("TLS error: {0}")]
    Tls(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {status}")]
    Http { status: StatusCode, body: Bytes },

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
