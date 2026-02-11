//! RemoteProxyClient - HTTP-based OAGW client using reqwest
//!
//! This client makes HTTP requests to OAGW's `/api/oagw/v1/proxy/{alias}/*` endpoint.
//! Used in production deployments where OAGW runs in a separate process.

use crate::error::ClientError;
use crate::types::{Body, ErrorSource, Request, Response, ResponseBody};
use futures::StreamExt;
use http::HeaderMap;
use std::time::Duration;
use tracing::{debug, trace};

/// RemoteProxyClient makes HTTP requests to OAGW proxy endpoint
pub(crate) struct RemoteProxyClient {
    /// Base URL of OAGW service (e.g., "https://oagw.internal.cf")
    oagw_base_url: String,

    /// Underlying HTTP client
    http_client: reqwest::Client,

    /// Authentication token for OAGW
    auth_token: String,
}

impl RemoteProxyClient {
    /// Create a new remote proxy client
    ///
    /// # Arguments
    /// * `base_url` - Base URL of OAGW service
    /// * `auth_token` - Authentication token for OAGW
    /// * `timeout` - Default request timeout
    ///
    /// # Errors
    /// Returns error if HTTP client creation fails
    pub fn new(
        base_url: String,
        auth_token: String,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(5))
            .user_agent("oagw-client/0.2.2")
            .build()
            .map_err(|e| ClientError::Config(format!("Failed to create HTTP client: {e}")))?;

        debug!(
            base_url = %base_url,
            timeout_secs = timeout.as_secs(),
            "Created RemoteProxyClient"
        );

        Ok(Self {
            oagw_base_url: base_url,
            http_client,
            auth_token,
        })
    }

    /// Execute a request through OAGW
    ///
    /// # Arguments
    /// * `alias` - Service alias (e.g., "openai", "anthropic")
    /// * `request` - Request to execute
    ///
    /// # Errors
    /// Returns error if:
    /// - Request build fails
    /// - Network connection fails
    /// - Timeout occurs
    pub async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        // Construct OAGW proxy URL: {base_url}/api/oagw/v1/proxy/{alias}{path}
        let url = format!(
            "{}/api/oagw/v1/proxy/{}{}",
            self.oagw_base_url,
            alias,
            request.path()
        );

        trace!(
            url = %url,
            method = %request.method(),
            "Executing remote proxy request"
        );

        // Build reqwest request
        let mut req_builder = self
            .http_client
            .request(request.method().clone(), &url)
            .header("Authorization", format!("Bearer {}", self.auth_token));

        // Forward headers from request
        for (name, value) in request.headers() {
            req_builder = req_builder.header(name, value);
        }

        // Set request timeout if specified
        if let Some(timeout) = request.timeout() {
            req_builder = req_builder.timeout(timeout);
        }

        // Set body
        match request.into_body() {
            Body::Empty => {
                // No body
            }
            Body::Bytes(b) => {
                req_builder = req_builder.body(b);
            }
            Body::Stream(_) => {
                return Err(ClientError::BuildError(
                    "Streaming request body not supported in RemoteProxyClient".into(),
                ));
            }
        }

        // Send request
        let resp = req_builder.send().await.map_err(|e| {
            if e.is_timeout() {
                ClientError::Timeout(format!("Request to {url} timed out"))
            } else if e.is_connect() {
                ClientError::Connection(format!("Connection to {url} failed: {e}"))
            } else {
                ClientError::from(e)
            }
        })?;

        // Extract response metadata
        let status = resp.status();
        let headers = resp.headers().clone();
        let error_source = parse_error_source_header(&headers);

        trace!(
            status = %status,
            error_source = ?error_source,
            "Received response from OAGW"
        );

        // Convert response to streaming body (consumer decides whether to buffer)
        let stream = resp.bytes_stream().map(|result| {
            result.map_err(|e| {
                if e.is_timeout() {
                    ClientError::Timeout("Stream read timeout".into())
                } else {
                    ClientError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
                }
            })
        });

        Ok(Response::new(
            status,
            headers,
            ResponseBody::Streaming(Box::pin(stream)),
            error_source,
        ))
    }
}

/// Parse error source from X-OAGW-Error-Source header
fn parse_error_source_header(headers: &HeaderMap) -> ErrorSource {
    headers
        .get("x-oagw-error-source")
        .and_then(|v| v.to_str().ok())
        .map(ErrorSource::from_header)
        .unwrap_or(ErrorSource::Unknown)
}

impl std::fmt::Debug for RemoteProxyClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteProxyClient")
            .field("oagw_base_url", &self.oagw_base_url)
            .field("auth_token", &"[REDACTED]")
            .finish()
    }
}
