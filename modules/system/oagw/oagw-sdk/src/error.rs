use serde::{Deserialize, Serialize};
use http::StatusCode;
use bytes::Bytes;

// ---------------------------------------------------------------------------
// RFC 9457 Problem Details
// ---------------------------------------------------------------------------

/// RFC 9457 Problem Details for HTTP APIs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProblemDetails {
    /// GTS error type identifier.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Human-readable summary.
    pub title: String,
    /// HTTP status code.
    pub status: u16,
    /// Occurrence-specific explanation.
    pub detail: String,
    /// Request URI.
    pub instance: String,
}

// ---------------------------------------------------------------------------
// GTS error type constants
// ---------------------------------------------------------------------------

pub const ERR_VALIDATION: &str = "gts.x.core.errors.err.v1~x.oagw.validation.error.v1";
pub const ERR_MISSING_TARGET_HOST: &str =
    "gts.x.core.errors.err.v1~x.oagw.routing.missing_target_host.v1";
pub const ERR_INVALID_TARGET_HOST: &str =
    "gts.x.core.errors.err.v1~x.oagw.routing.invalid_target_host.v1";
pub const ERR_UNKNOWN_TARGET_HOST: &str =
    "gts.x.core.errors.err.v1~x.oagw.routing.unknown_target_host.v1";
pub const ERR_AUTH_FAILED: &str = "gts.x.core.errors.err.v1~x.oagw.auth.failed.v1";
pub const ERR_ROUTE_NOT_FOUND: &str = "gts.x.core.errors.err.v1~x.oagw.route.not_found.v1";
pub const ERR_PAYLOAD_TOO_LARGE: &str = "gts.x.core.errors.err.v1~x.oagw.payload.too_large.v1";
pub const ERR_RATE_LIMIT_EXCEEDED: &str = "gts.x.core.errors.err.v1~x.oagw.rate_limit.exceeded.v1";
pub const ERR_SECRET_NOT_FOUND: &str = "gts.x.core.errors.err.v1~x.oagw.secret.not_found.v1";
pub const ERR_DOWNSTREAM: &str = "gts.x.core.errors.err.v1~x.oagw.downstream.error.v1";
pub const ERR_PROTOCOL: &str = "gts.x.core.errors.err.v1~x.oagw.protocol.error.v1";
pub const ERR_UPSTREAM_DISABLED: &str =
    "gts.x.core.errors.err.v1~x.oagw.routing.upstream_disabled.v1";
pub const ERR_CONNECTION_TIMEOUT: &str = "gts.x.core.errors.err.v1~x.oagw.timeout.connection.v1";
pub const ERR_REQUEST_TIMEOUT: &str = "gts.x.core.errors.err.v1~x.oagw.timeout.request.v1";

// ---------------------------------------------------------------------------
// OAGW error enum
// ---------------------------------------------------------------------------

/// Gateway-originated error with all information needed to produce a Problem Details response.
#[derive(Debug, Clone, thiserror::Error)]
pub enum OagwError {
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

impl OagwError {
    /// GTS error type identifier.
    #[must_use]
    pub fn gts_type(&self) -> &str {
        match self {
            Self::ValidationError { .. } => ERR_VALIDATION,
            Self::MissingTargetHost { .. } => ERR_MISSING_TARGET_HOST,
            Self::InvalidTargetHost { .. } => ERR_INVALID_TARGET_HOST,
            Self::UnknownTargetHost { .. } => ERR_UNKNOWN_TARGET_HOST,
            Self::AuthenticationFailed { .. } => ERR_AUTH_FAILED,
            Self::RouteNotFound { .. } => ERR_ROUTE_NOT_FOUND,
            Self::PayloadTooLarge { .. } => ERR_PAYLOAD_TOO_LARGE,
            Self::RateLimitExceeded { .. } => ERR_RATE_LIMIT_EXCEEDED,
            Self::SecretNotFound { .. } => ERR_SECRET_NOT_FOUND,
            Self::DownstreamError { .. } => ERR_DOWNSTREAM,
            Self::ProtocolError { .. } => ERR_PROTOCOL,
            Self::UpstreamDisabled { .. } => ERR_UPSTREAM_DISABLED,
            Self::ConnectionTimeout { .. } => ERR_CONNECTION_TIMEOUT,
            Self::RequestTimeout { .. } => ERR_REQUEST_TIMEOUT,
        }
    }

    /// HTTP status code for this error.
    #[must_use]
    pub fn status(&self) -> u16 {
        match self {
            Self::ValidationError { .. }
            | Self::MissingTargetHost { .. }
            | Self::InvalidTargetHost { .. }
            | Self::UnknownTargetHost { .. } => 400,
            Self::AuthenticationFailed { .. } => 401,
            Self::RouteNotFound { .. } => 404,
            Self::PayloadTooLarge { .. } => 413,
            Self::RateLimitExceeded { .. } => 429,
            Self::SecretNotFound { .. } => 500,
            Self::DownstreamError { .. } | Self::ProtocolError { .. } => 502,
            Self::UpstreamDisabled { .. } => 503,
            Self::ConnectionTimeout { .. } | Self::RequestTimeout { .. } => 504,
        }
    }

    /// Human-readable error title.
    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::ValidationError { .. } => "Validation Error",
            Self::MissingTargetHost { .. } => "Missing Target Host",
            Self::InvalidTargetHost { .. } => "Invalid Target Host",
            Self::UnknownTargetHost { .. } => "Unknown Target Host",
            Self::AuthenticationFailed { .. } => "Authentication Failed",
            Self::RouteNotFound { .. } => "Route Not Found",
            Self::PayloadTooLarge { .. } => "Payload Too Large",
            Self::RateLimitExceeded { .. } => "Rate Limit Exceeded",
            Self::SecretNotFound { .. } => "Secret Not Found",
            Self::DownstreamError { .. } => "Downstream Error",
            Self::ProtocolError { .. } => "Protocol Error",
            Self::UpstreamDisabled { .. } => "Upstream Disabled",
            Self::ConnectionTimeout { .. } => "Connection Timeout",
            Self::RequestTimeout { .. } => "Request Timeout",
        }
    }

    fn instance(&self) -> &str {
        match self {
            Self::ValidationError { instance, .. }
            | Self::MissingTargetHost { instance, .. }
            | Self::InvalidTargetHost { instance, .. }
            | Self::UnknownTargetHost { instance, .. }
            | Self::AuthenticationFailed { instance, .. }
            | Self::RouteNotFound { instance, .. }
            | Self::PayloadTooLarge { instance, .. }
            | Self::RateLimitExceeded { instance, .. }
            | Self::SecretNotFound { instance, .. }
            | Self::DownstreamError { instance, .. }
            | Self::ProtocolError { instance, .. }
            | Self::UpstreamDisabled { instance, .. }
            | Self::ConnectionTimeout { instance, .. }
            | Self::RequestTimeout { instance, .. } => instance,
        }
    }

    /// Convert to RFC 9457 Problem Details.
    #[must_use]
    pub fn to_problem_details(&self) -> ProblemDetails {
        ProblemDetails {
            error_type: self.gts_type().to_string(),
            title: self.title().to_string(),
            status: self.status(),
            detail: self.to_string(),
            instance: self.instance().to_string(),
        }
    }
}

// ===========================================================================
// Client error enum
// ===========================================================================

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_error_produces_correct_problem_details() {
        let err = OagwError::ValidationError {
            detail: "missing required field 'server'".into(),
            instance: "/oagw/v1/upstreams".into(),
        };
        let pd = err.to_problem_details();
        assert_eq!(pd.status, 400);
        assert_eq!(pd.error_type, ERR_VALIDATION);
        assert_eq!(pd.title, "Validation Error");
        assert!(pd.detail.contains("missing required field"));
        assert_eq!(pd.instance, "/oagw/v1/upstreams");
    }

    #[test]
    fn rate_limit_exceeded_produces_429() {
        let err = OagwError::RateLimitExceeded {
            detail: "rate limit exceeded for upstream".into(),
            instance: "/oagw/v1/proxy/api.openai.com/v1/chat/completions".into(),
            retry_after_secs: Some(30),
        };
        let pd = err.to_problem_details();
        assert_eq!(pd.status, 429);
        assert_eq!(pd.error_type, ERR_RATE_LIMIT_EXCEEDED);
    }

    #[test]
    fn route_not_found_produces_404() {
        let err = OagwError::RouteNotFound {
            instance: "/oagw/v1/proxy/unknown.host/path".into(),
        };
        let pd = err.to_problem_details();
        assert_eq!(pd.status, 404);
        assert_eq!(pd.error_type, ERR_ROUTE_NOT_FOUND);
    }

    #[test]
    fn all_error_types_produce_valid_json() {
        let errors: Vec<OagwError> = vec![
            OagwError::ValidationError {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::MissingTargetHost {
                instance: "/test".into(),
            },
            OagwError::InvalidTargetHost {
                instance: "/test".into(),
            },
            OagwError::UnknownTargetHost {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::AuthenticationFailed {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::RouteNotFound {
                instance: "/test".into(),
            },
            OagwError::PayloadTooLarge {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::RateLimitExceeded {
                detail: "test".into(),
                instance: "/test".into(),
                retry_after_secs: None,
            },
            OagwError::SecretNotFound {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::DownstreamError {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::ProtocolError {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::UpstreamDisabled {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::ConnectionTimeout {
                detail: "test".into(),
                instance: "/test".into(),
            },
            OagwError::RequestTimeout {
                detail: "test".into(),
                instance: "/test".into(),
            },
        ];
        for err in &errors {
            let pd = err.to_problem_details();
            let json = serde_json::to_string(&pd).unwrap();
            let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
            assert!(parsed.get("type").is_some(), "missing 'type' for {err:?}");
            assert!(
                parsed.get("status").is_some(),
                "missing 'status' for {err:?}"
            );
            assert!(parsed.get("title").is_some(), "missing 'title' for {err:?}");
            assert!(
                parsed.get("detail").is_some(),
                "missing 'detail' for {err:?}"
            );
            assert!(
                parsed.get("instance").is_some(),
                "missing 'instance' for {err:?}"
            );
        }
    }

    #[test]
    fn problem_details_serde_round_trip() {
        let pd = ProblemDetails {
            error_type: ERR_VALIDATION.into(),
            title: "Validation Error".into(),
            status: 400,
            detail: "test detail".into(),
            instance: "/test".into(),
        };
        let json = serde_json::to_string(&pd).unwrap();
        let pd2: ProblemDetails = serde_json::from_str(&json).unwrap();
        assert_eq!(pd, pd2);
    }
    
    #[test]
    fn test_client_error_display() {
        let err = ClientError::BuildError("test error".into());
        assert_eq!(err.to_string(), "Request build error: test error");
    }

    #[test]
    fn test_client_error_build_error() {
        let err = ClientError::BuildError("invalid header".into());
        assert_eq!(err.to_string(), "Request build error: invalid header");
    }

    #[test]
    fn test_client_error_connection() {
        let err = ClientError::Connection("connection refused".into());
        assert_eq!(err.to_string(), "Connection error: connection refused");
    }

    #[test]
    fn test_client_error_timeout() {
        let err = ClientError::Timeout("request timeout".into());
        assert_eq!(err.to_string(), "Timeout: request timeout");
    }

    #[test]
    fn test_client_error_tls() {
        let err = ClientError::Tls("certificate validation failed".into());
        assert_eq!(err.to_string(), "TLS error: certificate validation failed");
    }

    #[test]
    fn test_client_error_protocol() {
        let err = ClientError::Protocol("invalid HTTP version".into());
        assert_eq!(err.to_string(), "Protocol error: invalid HTTP version");
    }

    #[test]
    fn test_client_error_connection_closed() {
        let err = ClientError::ConnectionClosed;
        assert_eq!(err.to_string(), "Connection closed");
    }

    #[test]
    fn test_client_error_invalid_response() {
        let err = ClientError::InvalidResponse("malformed JSON".into());
        assert_eq!(err.to_string(), "Invalid response: malformed JSON");
    }

    #[test]
    fn test_client_error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk error");
        let err = ClientError::from(io_err);
        assert!(matches!(err, ClientError::Io(_)));
        assert!(err.to_string().contains("disk error"));
    }

    #[test]
    fn test_client_error_http() {
        let err = ClientError::Http {
            status: StatusCode::NOT_FOUND,
            body: Bytes::from("not found"),
        };
        assert_eq!(err.to_string(), "HTTP error: 404 Not Found");
    }

    #[test]
    fn test_client_error_serialization() {
        let json_err = serde_json::from_str::<serde_json::Value>("{invalid json}").unwrap_err();
        let err = ClientError::from(json_err);
        assert!(matches!(err, ClientError::Serialization(_)));
    }

    #[test]
    fn test_client_error_debug() {
        let err = ClientError::BuildError("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("BuildError"));
        assert!(debug.contains("test"));
    }
}
