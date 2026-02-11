//! Tests for error handling

use bytes::Bytes;
use http::StatusCode;
use oagw_sdk::{ClientError, ErrorSource};

#[test]
fn test_error_source_from_header() {
    assert_eq!(ErrorSource::from_header("gateway"), ErrorSource::Gateway);
    assert_eq!(ErrorSource::from_header("upstream"), ErrorSource::Upstream);
    assert_eq!(ErrorSource::from_header("unknown"), ErrorSource::Unknown);
    assert_eq!(ErrorSource::from_header("invalid"), ErrorSource::Unknown);
    assert_eq!(ErrorSource::from_header(""), ErrorSource::Unknown);
}

#[test]
fn test_error_source_predicates() {
    let gateway = ErrorSource::Gateway;
    assert!(gateway.is_gateway());
    assert!(!gateway.is_upstream());
    assert!(!gateway.is_unknown());

    let upstream = ErrorSource::Upstream;
    assert!(!upstream.is_gateway());
    assert!(upstream.is_upstream());
    assert!(!upstream.is_unknown());

    let unknown = ErrorSource::Unknown;
    assert!(!unknown.is_gateway());
    assert!(!unknown.is_upstream());
    assert!(unknown.is_unknown());
}

#[test]
fn test_error_source_default() {
    let default = ErrorSource::default();
    assert_eq!(default, ErrorSource::Unknown);
}

#[test]
fn test_client_error_is_connection() {
    let conn_err = ClientError::Connection("test".into());
    assert!(conn_err.is_connection());

    let closed_err = ClientError::ConnectionClosed;
    assert!(closed_err.is_connection());

    let timeout_err = ClientError::Timeout("test".into());
    assert!(!timeout_err.is_connection());
}

#[test]
fn test_client_error_is_timeout() {
    let timeout_err = ClientError::Timeout("test".into());
    assert!(timeout_err.is_timeout());

    let conn_err = ClientError::Connection("test".into());
    assert!(!conn_err.is_timeout());
}

#[test]
fn test_client_error_is_tls() {
    let tls_err = ClientError::Tls("test".into());
    assert!(tls_err.is_tls());

    let conn_err = ClientError::Connection("test".into());
    assert!(!conn_err.is_tls());
}

#[test]
fn test_client_error_is_http() {
    let http_err = ClientError::Http {
        status: StatusCode::NOT_FOUND,
        body: Bytes::new(),
    };
    assert!(http_err.is_http());

    let conn_err = ClientError::Connection("test".into());
    assert!(!conn_err.is_http());
}

#[test]
fn test_client_error_http_status() {
    let http_err = ClientError::Http {
        status: StatusCode::NOT_FOUND,
        body: Bytes::new(),
    };
    assert_eq!(http_err.http_status(), Some(StatusCode::NOT_FOUND));

    let conn_err = ClientError::Connection("test".into());
    assert_eq!(conn_err.http_status(), None);
}

#[test]
fn test_client_error_is_retryable() {
    // Retryable errors
    assert!(ClientError::Connection("test".into()).is_retryable());
    assert!(ClientError::Timeout("test".into()).is_retryable());
    assert!(ClientError::ConnectionClosed.is_retryable());
    assert!(ClientError::Io(std::io::Error::new(
        std::io::ErrorKind::Other,
        "test"
    ))
    .is_retryable());

    // Server errors are retryable
    assert!(ClientError::Http {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        body: Bytes::new(),
    }
    .is_retryable());

    assert!(ClientError::Http {
        status: StatusCode::SERVICE_UNAVAILABLE,
        body: Bytes::new(),
    }
    .is_retryable());

    // Rate limit is retryable
    assert!(ClientError::Http {
        status: StatusCode::TOO_MANY_REQUESTS,
        body: Bytes::new(),
    }
    .is_retryable());

    // Non-retryable errors
    assert!(!ClientError::BuildError("test".into()).is_retryable());
    assert!(!ClientError::Tls("test".into()).is_retryable());
    assert!(!ClientError::Protocol("test".into()).is_retryable());
    assert!(!ClientError::InvalidResponse("test".into()).is_retryable());
    assert!(!ClientError::Config("test".into()).is_retryable());

    // Client errors are not retryable
    assert!(!ClientError::Http {
        status: StatusCode::BAD_REQUEST,
        body: Bytes::new(),
    }
    .is_retryable());

    assert!(!ClientError::Http {
        status: StatusCode::NOT_FOUND,
        body: Bytes::new(),
    }
    .is_retryable());

    assert!(!ClientError::Http {
        status: StatusCode::UNAUTHORIZED,
        body: Bytes::new(),
    }
    .is_retryable());
}

#[test]
fn test_error_display() {
    let err = ClientError::Connection("network error".into());
    assert!(err.to_string().contains("Connection error"));
    assert!(err.to_string().contains("network error"));

    let err = ClientError::Timeout("request timed out".into());
    assert!(err.to_string().contains("Timeout"));

    let err = ClientError::Http {
        status: StatusCode::NOT_FOUND,
        body: Bytes::new(),
    };
    assert!(err.to_string().contains("404"));
}
