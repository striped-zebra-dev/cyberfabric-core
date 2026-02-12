//! OAGW Client implementations

mod remote_proxy;
mod shared_process;

pub use remote_proxy::RemoteProxyClient;
pub use shared_process::SharedProcessClient;

use async_trait::async_trait;
use modkit::context::ModuleCtx;
use oagw_sdk::client::*;
use oagw_sdk::service::DataPlaneService;
use tracing::{debug, info};
use uuid::Uuid;

/// Main OAGW client - deployment-agnostic
///
/// This client automatically dispatches to the appropriate implementation
/// (SharedProcessClient or RemoteProxyClient) based on configuration.
pub struct OagwClient {
    inner: OagwClientImpl,
}

enum OagwClientImpl {
    SharedProcess(SharedProcessClient),
    RemoteProxy(RemoteProxyClient),
}

impl OagwClient {
    /// Create client from ModuleCtx, automatically detecting deployment mode
    ///
    /// This is the recommended way to create an OagwClient in consuming modules.
    ///
    /// Environment variables:
    /// - `OAGW_MODE`: "shared" or "remote" (defaults to "shared")
    /// - `OAGW_BASE_URL`: Base URL for remote mode (defaults to "http://localhost:8080")
    /// - `OAGW_AUTH_TOKEN`: Auth token for remote mode (required for remote)
    ///
    /// # Arguments
    ///
    /// * `ctx` - ModuleCtx to resolve DataPlaneService from ClientHub (for SharedProcess mode)
    /// * `tenant_id` - Tenant ID for this client instance
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oagw::client::OagwClient;
    ///
    /// # async fn example(ctx: &modkit::ModuleCtx, tenant_id: uuid::Uuid) -> Result<(), Box<dyn std::error::Error>> {
    /// let client = OagwClient::from_ctx(ctx, tenant_id)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_ctx(ctx: &ModuleCtx, tenant_id: Uuid) -> Result<Self, ClientError> {
        match std::env::var("OAGW_MODE").as_deref() {
            Ok("remote") => {
                // Remote proxy mode
                let base_url = std::env::var("OAGW_BASE_URL")
                    .unwrap_or_else(|_| "http://localhost:8080".to_string());
                let auth_token = std::env::var("OAGW_AUTH_TOKEN").map_err(|_| {
                    ClientError::BuildError("OAGW_AUTH_TOKEN required for remote mode".into())
                })?;

                info!("Creating OagwClient in RemoteProxy mode (base_url={})", base_url);
                Self::remote_proxy(base_url, auth_token, std::time::Duration::from_secs(30))
            }
            Ok("shared") | Ok(_) | Err(_) => {
                // Default to shared-process mode
                info!("Creating OagwClient in SharedProcess mode");

                // Get DataPlaneService from ClientHub
                let data_plane = ctx
                    .client_hub()
                    .get::<dyn DataPlaneService>()
                    .map_err(|e| {
                        ClientError::BuildError(format!(
                            "Failed to get DataPlaneService from ClientHub: {}",
                            e
                        ))
                    })?;

                Self::shared_process(data_plane, tenant_id)
            }
        }
    }

    /// Create client from configuration
    ///
    /// # Example
    ///
    /// ```no_run
    /// use oagw::client::OagwClient;
    /// use oagw_sdk::client::OagwClientConfig;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = OagwClientConfig::remote_proxy(
    ///     "http://localhost:8080".to_string(),
    ///     "token".to_string(),
    /// );
    /// let client = OagwClient::from_config(config, uuid::Uuid::nil())?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_config(config: OagwClientConfig, tenant_id: Uuid) -> Result<Self, ClientError> {
        let inner = match config.mode {
            ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout,
            } => {
                info!("Creating OagwClient in RemoteProxy mode");
                OagwClientImpl::RemoteProxy(RemoteProxyClient::new(
                    base_url, auth_token, timeout,
                )?)
            }
            ClientMode::SharedProcess { data_plane } => {
                info!("Creating OagwClient in SharedProcess mode");
                OagwClientImpl::SharedProcess(SharedProcessClient::new(data_plane, tenant_id)?)
            }
        };

        Ok(Self { inner })
    }

    /// Create client in SharedProcess mode with explicit tenant ID
    ///
    /// This is useful for testing or when tenant ID is known at client creation time.
    pub fn shared_process(
        data_plane: std::sync::Arc<dyn oagw_sdk::service::DataPlaneService>,
        tenant_id: Uuid,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            inner: OagwClientImpl::SharedProcess(SharedProcessClient::new(
                data_plane, tenant_id,
            )?),
        })
    }

    /// Create client in RemoteProxy mode
    pub fn remote_proxy(
        base_url: String,
        auth_token: String,
        timeout: std::time::Duration,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            inner: OagwClientImpl::RemoteProxy(RemoteProxyClient::new(
                base_url, auth_token, timeout,
            )?),
        })
    }
}

#[async_trait]
impl OagwClientApi for OagwClient {
    async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        match &self.inner {
            OagwClientImpl::SharedProcess(c) => {
                debug!("Routing request through SharedProcessClient");
                c.execute(alias, request).await
            }
            OagwClientImpl::RemoteProxy(c) => {
                debug!("Routing request through RemoteProxyClient");
                c.execute(alias, request).await
            }
        }
    }

    async fn websocket(&self, alias: &str, request: Request) -> Result<WebSocketConn, ClientError> {
        match &self.inner {
            OagwClientImpl::SharedProcess(c) => c.websocket(alias, request).await,
            OagwClientImpl::RemoteProxy(c) => c.websocket(alias, request).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_remote_proxy_client_creation() {
        let client = OagwClient::remote_proxy(
            "http://localhost:8080".to_string(),
            "test-token".to_string(),
            Duration::from_secs(30),
        );

        assert!(client.is_ok());
    }

    #[test]
    fn test_config_based_creation() {
        // Test RemoteProxy config
        let config = OagwClientConfig::remote_proxy(
            "http://localhost:8080".to_string(),
            "test-token".to_string(),
        );

        let client = OagwClient::from_config(config, Uuid::nil());
        assert!(client.is_ok());
    }
}
