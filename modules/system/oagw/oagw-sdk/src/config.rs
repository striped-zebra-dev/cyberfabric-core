//! Configuration for OAGW client
//!
//! Supports two deployment modes:
//! - SharedProcess: Direct function calls to Control Plane (development)
//! - RemoteProxy: HTTP requests to OAGW proxy endpoint (production)

use crate::error::ClientError;
use oagw_core::service::ControlPlaneService;
use std::sync::Arc;
use std::time::Duration;

/// OAGW client configuration
#[derive(Clone)]
pub struct OagwClientConfig {
    /// Deployment mode (SharedProcess or RemoteProxy)
    pub mode: ClientMode,

    /// Default timeout for requests (can be overridden per request)
    pub default_timeout: Duration,
}

/// Client deployment mode
#[derive(Clone)]
pub enum ClientMode {
    /// OAGW in same process - direct function calls
    ///
    /// Used in development or when OAGW Control Plane is embedded in the same process.
    /// Provides zero serialization overhead.
    SharedProcess {
        /// Control Plane service instance
        control_plane: Arc<dyn ControlPlaneService>,
    },

    /// OAGW in separate process - HTTP calls to proxy endpoint
    ///
    /// Used in production deployments where OAGW runs as a separate service.
    RemoteProxy {
        /// Base URL of OAGW service (e.g., "https://oagw.internal.cf")
        base_url: String,

        /// Authentication token for OAGW
        auth_token: String,

        /// Request timeout
        timeout: Duration,
    },
}

impl OagwClientConfig {
    /// Create config from environment variables
    ///
    /// Environment variables:
    /// - `OAGW_MODE`: "shared" or "remote" (default: "remote")
    /// - `OAGW_BASE_URL`: Base URL for remote mode (default: "https://oagw.internal.cf")
    /// - `OAGW_AUTH_TOKEN`: Auth token for remote mode (required for remote mode)
    ///
    /// # Errors
    /// Returns error if:
    /// - Required environment variables are missing
    /// - Environment variables have invalid values
    pub fn from_env() -> Result<Self, ClientError> {
        let mode_str = std::env::var("OAGW_MODE").unwrap_or_else(|_| "remote".to_string());

        let mode = match mode_str.as_str() {
            "shared" => {
                // In shared-process mode, control plane is injected by modkit
                // For now, return error as DI is not yet implemented
                return Err(ClientError::Config(
                    "SharedProcess mode requires Control Plane dependency injection (not yet implemented)".into(),
                ));
            }
            "remote" | _ => {
                // Default to remote mode
                let base_url = std::env::var("OAGW_BASE_URL")
                    .unwrap_or_else(|_| "https://oagw.internal.cf".to_string());

                let auth_token = std::env::var("OAGW_AUTH_TOKEN").map_err(|_| {
                    ClientError::Config(
                        "OAGW_AUTH_TOKEN environment variable is required for remote mode".into(),
                    )
                })?;

                let timeout = std::env::var("OAGW_TIMEOUT_SECS")
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or_else(|| Duration::from_secs(30));

                ClientMode::RemoteProxy {
                    base_url,
                    auth_token,
                    timeout,
                }
            }
        };

        Ok(Self {
            mode,
            default_timeout: Duration::from_secs(30),
        })
    }

    /// Create a new config for SharedProcess mode
    ///
    /// # Arguments
    /// * `control_plane` - Control Plane service instance
    #[must_use]
    pub fn shared_process(control_plane: Arc<dyn ControlPlaneService>) -> Self {
        Self {
            mode: ClientMode::SharedProcess { control_plane },
            default_timeout: Duration::from_secs(30),
        }
    }

    /// Create a new config for RemoteProxy mode
    ///
    /// # Arguments
    /// * `base_url` - Base URL of OAGW service
    /// * `auth_token` - Authentication token
    #[must_use]
    pub fn remote_proxy(base_url: String, auth_token: String) -> Self {
        Self {
            mode: ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout: Duration::from_secs(30),
            },
            default_timeout: Duration::from_secs(30),
        }
    }

    /// Set the default timeout
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;

        // Also update the mode-specific timeout for RemoteProxy
        if let ClientMode::RemoteProxy {
            base_url,
            auth_token,
            ..
        } = self.mode
        {
            self.mode = ClientMode::RemoteProxy {
                base_url,
                auth_token,
                timeout,
            };
        }

        self
    }

    /// Check if using SharedProcess mode
    #[must_use]
    pub const fn is_shared_process(&self) -> bool {
        matches!(self.mode, ClientMode::SharedProcess { .. })
    }

    /// Check if using RemoteProxy mode
    #[must_use]
    pub const fn is_remote_proxy(&self) -> bool {
        matches!(self.mode, ClientMode::RemoteProxy { .. })
    }
}

impl std::fmt::Debug for OagwClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mode_str = match &self.mode {
            ClientMode::SharedProcess { .. } => "SharedProcess",
            ClientMode::RemoteProxy { base_url, .. } => {
                return f
                    .debug_struct("OagwClientConfig")
                    .field("mode", &"RemoteProxy")
                    .field("base_url", base_url)
                    .field("auth_token", &"[REDACTED]")
                    .field("default_timeout", &self.default_timeout)
                    .finish();
            }
        };

        f.debug_struct("OagwClientConfig")
            .field("mode", &mode_str)
            .field("default_timeout", &self.default_timeout)
            .finish()
    }
}
