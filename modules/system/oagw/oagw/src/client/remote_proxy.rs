//! RemoteProxyClient - HTTP requests to OAGW proxy endpoint

use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::StreamExt;
use http::StatusCode;
use oagw_sdk::client::*;
use oagw_sdk::service::ErrorSource;
use tracing::{debug, error, trace};

/// Remote proxy client - HTTP requests to OAGW proxy endpoint
///
/// This client is used when OAGW runs in a separate process. It makes
/// HTTP requests to the OAGW `/api/oagw/v1/proxy/{alias}/*` endpoint.
pub struct RemoteProxyClient {
    oagw_base_url: String,
    http_client: reqwest::Client,
    auth_token: String,
}

impl RemoteProxyClient {
    /// Create a new RemoteProxyClient
    ///
    /// # Arguments
    ///
    /// * `base_url` - OAGW base URL (e.g., "http://localhost:8080")
    /// * `auth_token` - Authentication token for OAGW
    /// * `timeout` - Default request timeout
    pub fn new(
        base_url: String,
        auth_token: String,
        timeout: Duration,
    ) -> Result<Self, ClientError> {
        debug!(
            "Creating RemoteProxyClient with base_url={}, timeout={:?}",
            base_url, timeout
        );

        let http_client = reqwest::Client::builder()
            .timeout(timeout)
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| ClientError::BuildError(format!("Failed to build HTTP client: {}", e)))?;

        Ok(Self {
            oagw_base_url: base_url,
            http_client,
            auth_token,
        })
    }

    /// Map reqwest errors to ClientError
    fn map_reqwest_error(&self, error: reqwest::Error) -> ClientError {
        if error.is_timeout() {
            ClientError::Timeout(error.to_string())
        } else if error.is_connect() {
            ClientError::Connection(error.to_string())
        } else if error.is_decode() {
            ClientError::InvalidResponse(error.to_string())
        } else {
            ClientError::Protocol(error.to_string())
        }
    }
}

#[async_trait]
impl OagwClientApi for RemoteProxyClient {
    async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        // Build URL: http://localhost:8080/api/oagw/v1/proxy/{alias}{path}
        let url = format!(
            "{}/api/oagw/v1/proxy/{}{}",
            self.oagw_base_url,
            alias,
            request.path()
        );

        debug!(
            "RemoteProxyClient: {} {} (alias={})",
            request.method(),
            url,
            alias
        );

        // Build request - need to convert Method from http 1.3 to reqwest's http 0.2
        let method_str = request.method().as_str();
        let reqwest_method = reqwest::Method::from_bytes(method_str.as_bytes())
            .map_err(|e| ClientError::BuildError(format!("Invalid HTTP method: {}", e)))?;

        let mut req_builder = self
            .http_client
            .request(reqwest_method, &url)
            .header("Authorization", format!("Bearer {}", self.auth_token));

        // Forward all headers from original request
        // Convert from http 1.3 to reqwest's http 0.2 by going through strings
        for (name, value) in request.headers() {
            if let Ok(value_str) = value.to_str() {
                req_builder = req_builder.header(name.as_str(), value_str);
            }
        }

        // Set body
        match request.into_body() {
            Body::Empty => {}
            Body::Bytes(b) => {
                trace!("Request body: {} bytes", b.len());
                req_builder = req_builder.body(b.to_vec());
            }
            Body::Stream(_) => {
                return Err(ClientError::BuildError(
                    "Streaming request body not supported for plain requests".into(),
                ));
            }
        }

        // Send request
        let resp = req_builder
            .send()
            .await
            .map_err(|e| self.map_reqwest_error(e))?;

        // Convert reqwest types to http 1.3 types (our SDK uses http 1.3)
        let status_code = resp.status().as_u16();
        let status = StatusCode::from_u16(status_code)
            .map_err(|e| ClientError::InvalidResponse(format!("Invalid status code: {}", e)))?;

        // Convert headers from reqwest (http 1.4) to http 1.3
        let mut headers = http::HeaderMap::new();
        for (name, value) in resp.headers() {
            if let Ok(header_name) = http::HeaderName::from_bytes(name.as_str().as_bytes()) {
                if let Ok(header_value) = http::HeaderValue::from_bytes(value.as_bytes()) {
                    headers.insert(header_name, header_value);
                }
            }
        }

        debug!("RemoteProxyClient: Response status={}", status);

        // Parse X-OAGW-Error-Source header
        // Default to Gateway if header is missing or invalid
        let error_source = headers
            .get("x-oagw-error-source")
            .and_then(|v| v.to_str().ok())
            .map(|s| match s {
                "upstream" => ErrorSource::Upstream,
                _ => ErrorSource::Gateway,
            })
            .unwrap_or(ErrorSource::Gateway);

        trace!("Error source: {:?}", error_source);

        // Always return as streaming - consumer decides if they want to buffer
        // This allows flexibility: .bytes() for buffered, .into_stream() for streaming
        let stream = resp.bytes_stream().map(|result: Result<Bytes, reqwest::Error>| {
            result.map_err(|e| {
                ClientError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
            })
        });

        Ok(Response::from_stream(
            status,
            headers,
            Box::pin(stream),
            error_source,
        ))
    }

    async fn websocket(&self, _alias: &str, _request: Request) -> Result<WebSocketConn, ClientError> {
        // Phase 1 implementation
        error!("WebSocket not yet implemented for RemoteProxyClient");
        Err(ClientError::BuildError(
            "WebSocket not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_proxy_client_creation() {
        let client = RemoteProxyClient::new(
            "http://localhost:8080".to_string(),
            "test-token".to_string(),
            Duration::from_secs(30),
        );

        assert!(client.is_ok());
    }
}
