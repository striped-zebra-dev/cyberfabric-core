//! SharedProcessClient - Direct function call client for OAGW
//!
//! This client makes direct function calls to OAGW's Control Plane service
//! when running in the same process. Zero serialization overhead.

use crate::error::ClientError;
use crate::types::{Body, ErrorSource, Request, Response, ResponseBody};
use futures::StreamExt;
use http::HeaderMap;
use oagw_core::service::{Body as ProxyBody, ControlPlaneService, ProxyRequest};
use std::sync::Arc;
use tracing::{debug, trace};

/// SharedProcessClient makes direct function calls to OAGW Control Plane
pub(crate) struct SharedProcessClient {
    /// Control Plane service reference
    control_plane: Arc<dyn ControlPlaneService>,
}

impl SharedProcessClient {
    /// Create a new shared process client
    ///
    /// # Arguments
    /// * `control_plane` - Control Plane service instance
    ///
    /// # Errors
    /// Returns error if control plane is invalid (currently always succeeds)
    pub fn new(control_plane: Arc<dyn ControlPlaneService>) -> Result<Self, ClientError> {
        debug!("Created SharedProcessClient with direct Control Plane access");
        Ok(Self { control_plane })
    }

    /// Execute a request through Control Plane
    ///
    /// # Arguments
    /// * `alias` - Service alias (e.g., "openai", "anthropic")
    /// * `request` - Request to execute
    ///
    /// # Errors
    /// Returns error if:
    /// - Alias configuration not found
    /// - Control Plane service fails
    /// - Data Plane connection fails
    pub async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        trace!(
            alias = alias,
            method = %request.method(),
            path = request.path(),
            "Executing shared process request"
        );

        // Extract request fields before consuming it
        let method = request.method().clone();
        let path = request.path().to_string();
        let headers = request.headers().clone();

        // Convert SDK Body to oagw-core Body
        let proxy_body = match request.into_body() {
            Body::Empty => ProxyBody::Empty,
            Body::Bytes(b) => ProxyBody::Bytes(b),
            Body::Stream(_) => {
                return Err(ClientError::BuildError(
                    "Streaming request body not supported in SharedProcessClient".into(),
                ));
            }
        };

        // Convert SDK Request to ProxyRequest
        let proxy_request = ProxyRequest {
            alias: alias.to_string(),
            method,
            path,
            headers,
            body: proxy_body,
        };

        // Direct function call to Control Plane (zero serialization!)
        let proxy_response = self
            .control_plane
            .proxy_request(proxy_request)
            .await
            .map_err(|e| {
                // Convert oagw-core::service::Error to ClientError
                ClientError::Connection(format!("Control Plane error: {e}"))
            })?;

        trace!(
            status = %proxy_response.status,
            is_streaming = proxy_response.is_streaming,
            "Received response from Control Plane"
        );

        // Parse error source from headers
        let error_source = parse_error_source_header(&proxy_response.headers);

        // Convert response body based on streaming flag
        let body = if proxy_response.is_streaming {
            // Streaming response
            let stream = proxy_response
                .body_stream
                .ok_or_else(|| {
                    ClientError::Protocol(
                        "Response marked as streaming but no stream provided".into(),
                    )
                })?
                .map(|result| {
                    result.map_err(|e| {
                        ClientError::Connection(format!("Stream error: {e}"))
                    })
                });

            ResponseBody::Streaming(Box::pin(stream))
        } else {
            // Buffered response
            ResponseBody::Buffered(proxy_response.body)
        };

        Ok(Response::new(
            proxy_response.status,
            proxy_response.headers,
            body,
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

impl std::fmt::Debug for SharedProcessClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedProcessClient")
            .field("control_plane", &"<ControlPlaneService>")
            .finish()
    }
}
