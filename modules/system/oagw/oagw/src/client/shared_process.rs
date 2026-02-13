//! SharedProcessClient - Direct function calls to OAGW Data Plane

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use oagw_sdk::client::{OagwClientApi, WebSocketConn, DataPlaneService};
use oagw_sdk::request::Request;
use oagw_sdk::response::Response;
use oagw_sdk::error::{ClientError};
use oagw_sdk::api::{ProxyContext, ErrorSource};
use tracing::{debug, error, trace};
use uuid::Uuid;

/// Shared-process client - direct function calls to Data Plane
///
/// This client is used when OAGW runs in the same process as the consuming module.
/// It makes direct function calls to the Data Plane service with zero serialization overhead.
pub struct SharedProcessClient {
    data_plane: Arc<dyn DataPlaneService>,
}

impl SharedProcessClient {
    /// Create a new SharedProcessClient
    ///
    /// # Arguments
    ///
    /// * `data_plane` - Data plane service for executing proxy requests
    /// * `tenant_id` - Default tenant ID for all requests
    pub fn new(
        data_plane: Arc<dyn DataPlaneService>,
    ) -> Result<Self, ClientError> {
        debug!(
            "Creating SharedProcessClient"
        );

        Ok(Self {
            data_plane,
        })
    }
}

#[async_trait]
impl OagwClientApi for SharedProcessClient {
    async fn execute(&self, alias: &str, tenant_id: Uuid, request: Request) -> Result<Response, ClientError> {
        debug!(
            "SharedProcessClient: {} {} (alias={})",
            request.method(),
            request.path(),
            alias
        );

        // Extract request data before consuming it
        let method = request.method().clone();
        let path = request.path().to_string();
        let headers = request.headers().clone();

        // Convert Request to ProxyContext
        let body = request.into_body().into_bytes().await?;

        trace!("Request body: {} bytes", body.len());

        let ctx = ProxyContext {
            tenant_id,
            method,
            alias: alias.to_string(),
            path_suffix: path.clone(),
            query_params: vec![], // TODO: Parse query params from path
            headers,
            body,
            instance_uri: format!("/proxy/{}{}", alias, path),
        };

        // Direct function call to Data Plane (zero serialization)
        let proxy_response = self
            .data_plane
            .proxy_request(ctx)
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        debug!("SharedProcessClient: Response status={}", proxy_response.status);
        trace!("Error source: {:?}", proxy_response.error_source);

        // Convert ProxyResponse to Response
        // ProxyResponse.body is already a BodyStream
        let error_source = match proxy_response.error_source {
            ErrorSource::Gateway => ErrorSource::Gateway,
            ErrorSource::Upstream => ErrorSource::Upstream,
        };

        // Convert BodyStream to BoxStream<Result<Bytes, ClientError>>
        let stream = proxy_response.body.map(|result: Result<Bytes, oagw_sdk::api::BoxError>| {
            result.map_err(|e| {
                ClientError::Io(std::io::Error::new(std::io::ErrorKind::Other, e))
            })
        });

        Ok(Response::from_stream(
            proxy_response.status,
            proxy_response.headers,
            Box::pin(stream),
            error_source,
        ))
    }

    async fn websocket(&self, _alias: &str, _tenant_id: Uuid, _request: Request) -> Result<WebSocketConn, ClientError> {
        // Phase 1 implementation
        error!("WebSocket not yet implemented for SharedProcessClient");
        Err(ClientError::BuildError(
            "WebSocket not yet implemented".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    // Mock services would be needed for proper testing
    // Integration tests will cover this
}