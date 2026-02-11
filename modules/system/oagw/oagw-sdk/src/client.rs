//! OAGW Client - Main public API
//!
//! Provides deployment-agnostic HTTP client that routes requests through OAGW.
//! Application code is identical regardless of deployment mode (SharedProcess or RemoteProxy).

use crate::config::{ClientMode, OagwClientConfig};
use crate::error::ClientError;
use crate::impl_remote::RemoteProxyClient;
use crate::impl_shared::SharedProcessClient;
use crate::types::{Request, Response};
use tracing::debug;

/// OAGW HTTP Client
///
/// Routes all requests through OAGW (Outbound API Gateway).
/// Supports both SharedProcess (direct function calls) and RemoteProxy (HTTP) modes.
///
/// # Examples
///
/// ## Buffered Request
/// ```ignore
/// use oagw_sdk::{OagwClient, OagwClientConfig, Request, Method};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = OagwClientConfig::from_env()?;
/// let client = OagwClient::from_config(config)?;
///
/// let request = Request::builder()
///     .method(Method::GET)
///     .path("/v1/models")
///     .build()?;
///
/// let response = client.execute("openai", request).await?;
/// let data = response.json::<serde_json::Value>().await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Streaming SSE
/// ```ignore
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// # let client = oagw_sdk::OagwClient::from_config(oagw_sdk::OagwClientConfig::from_env()?)?;
/// let request = Request::builder()
///     .method(Method::POST)
///     .path("/v1/chat/completions")
///     .json(&json!({"model": "gpt-4", "stream": true}))?
///     .build()?;
///
/// let response = client.execute("openai", request).await?;
/// let mut sse = response.into_sse_stream();
///
/// while let Some(event) = sse.next_event().await? {
///     println!("{}", event.data);
/// }
/// # Ok(())
/// # }
/// ```
pub struct OagwClient {
    inner: OagwClientImpl,
}

/// Internal client implementation
///
/// Uses enum dispatch to route to SharedProcess or RemoteProxy implementation.
/// The enum dispatch is optimized away by the compiler (zero-cost abstraction).
enum OagwClientImpl {
    /// Direct function calls to Control Plane
    SharedProcess(SharedProcessClient),

    /// HTTP requests to OAGW proxy endpoint
    RemoteProxy(RemoteProxyClient),
}

impl OagwClient {
    /// Create a new OAGW client from configuration
    ///
    /// # Arguments
    /// * `config` - Client configuration (deployment mode, timeouts, etc.)
    ///
    /// # Errors
    /// Returns error if:
    /// - HTTP client creation fails (RemoteProxy mode)
    /// - Control Plane service is invalid (SharedProcess mode)
    ///
    /// # Examples
    /// ```ignore
    /// let config = OagwClientConfig::from_env()?;
    /// let client = OagwClient::from_config(config)?;
    /// ```
    pub fn from_config(config: OagwClientConfig) -> Result<Self, ClientError> {
        debug!(
            mode = if config.is_shared_process() {
                "SharedProcess"
            } else {
                "RemoteProxy"
            },
            "Creating OagwClient"
        );

        let inner = match config.mode {
            ClientMode::SharedProcess { control_plane } => {
                let client = SharedProcessClient::new(control_plane)?;
                OagwClientImpl::SharedProcess(client)
            }
            ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout,
            } => {
                let client = RemoteProxyClient::new(base_url, auth_token, timeout)?;
                OagwClientImpl::RemoteProxy(client)
            }
        };

        Ok(Self { inner })
    }

    /// Execute a request through OAGW
    ///
    /// # Arguments
    /// * `alias` - Service alias configured in OAGW (e.g., "openai", "anthropic", "unpkg")
    /// * `request` - HTTP request to execute
    ///
    /// # Returns
    /// Response from the upstream service, routed through OAGW
    ///
    /// # Errors
    /// Returns error if:
    /// - Alias configuration not found in OAGW
    /// - Network connection fails
    /// - Request timeout occurs
    /// - Upstream service returns error
    ///
    /// # Examples
    /// ```ignore
    /// let request = Request::builder()
    ///     .method(Method::GET)
    ///     .path("/v1/models")
    ///     .build()?;
    ///
    /// let response = client.execute("openai", request).await?;
    ///
    /// if response.error_source() == ErrorSource::Gateway {
    ///     eprintln!("OAGW gateway error");
    /// } else if response.error_source() == ErrorSource::Upstream {
    ///     eprintln!("OpenAI API error");
    /// }
    /// ```
    pub async fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        match &self.inner {
            OagwClientImpl::SharedProcess(client) => client.execute(alias, request).await,
            OagwClientImpl::RemoteProxy(client) => client.execute(alias, request).await,
        }
    }

    /// Get a reference to the blocking API
    ///
    /// Used for synchronous contexts like build scripts.
    ///
    /// # Examples
    /// ```ignore
    /// // In build.rs
    /// let client = OagwClient::from_config(config)?;
    /// let response = client.blocking().execute("unpkg", request)?;
    /// ```
    #[must_use]
    pub const fn blocking(&self) -> crate::blocking::BlockingClient<'_> {
        crate::blocking::BlockingClient::new(self)
    }
}

impl std::fmt::Debug for OagwClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode = match &self.inner {
            OagwClientImpl::SharedProcess(_) => "SharedProcess",
            OagwClientImpl::RemoteProxy(_) => "RemoteProxy",
        };

        f.debug_struct("OagwClient").field("mode", &mode).finish()
    }
}
