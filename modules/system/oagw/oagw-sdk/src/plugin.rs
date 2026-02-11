use http::HeaderMap;

// ---------------------------------------------------------------------------
// Plugin errors
// ---------------------------------------------------------------------------

/// Errors returned by plugin execution.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("secret not found: {0}")]
    SecretNotFound(String),
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("request rejected: {0}")]
    Rejected(String),
    #[error("plugin error: {0}")]
    Internal(String),
}

// ---------------------------------------------------------------------------
// Auth plugin
// ---------------------------------------------------------------------------

/// Context passed to authentication plugins.
pub struct AuthContext {
    /// Outbound request headers (mutable — plugin can inject credentials).
    pub headers: HeaderMap,
    /// Plugin-specific configuration from the upstream auth config.
    pub config: serde_json::Value,
}

/// Trait for authentication plugins that inject credentials into outbound requests.
#[async_trait::async_trait]
pub trait AuthPlugin: Send + Sync {
    /// Modify the outbound request headers to inject authentication credentials.
    ///
    /// # Errors
    /// Returns `PluginError` if credential resolution or injection fails.
    async fn authenticate(&self, ctx: &mut AuthContext) -> Result<(), PluginError>;
}

// ---------------------------------------------------------------------------
// Guard plugin (future use — not implemented in P1)
// ---------------------------------------------------------------------------

/// Read-only context passed to guard plugins.
pub struct GuardContext {
    pub method: String,
    pub path: String,
    pub headers: HeaderMap,
}

/// Trait for guard plugins that validate requests and enforce policies.
#[async_trait::async_trait]
pub trait GuardPlugin: Send + Sync {
    /// Validate the request. Return `Err(PluginError::Rejected)` to deny.
    ///
    /// # Errors
    /// Returns `PluginError` if validation fails.
    async fn guard(&self, ctx: &GuardContext) -> Result<(), PluginError>;
}

// ---------------------------------------------------------------------------
// Transform plugin (future use — not implemented in P1)
// ---------------------------------------------------------------------------

/// Mutable request context for transform plugins.
pub struct TransformRequestContext {
    pub headers: HeaderMap,
    pub body: bytes::Bytes,
}

/// Mutable response context for transform plugins.
pub struct TransformResponseContext {
    pub status: u16,
    pub headers: HeaderMap,
    pub body: bytes::Bytes,
}

/// Trait for transform plugins that modify request/response data.
#[async_trait::async_trait]
pub trait TransformPlugin: Send + Sync {
    /// Transform the outbound request before it is sent.
    ///
    /// # Errors
    /// Returns `PluginError` on failure.
    async fn on_request(&self, ctx: &mut TransformRequestContext) -> Result<(), PluginError>;

    /// Transform the upstream response before it is returned to the client.
    ///
    /// # Errors
    /// Returns `PluginError` on failure.
    async fn on_response(&self, ctx: &mut TransformResponseContext) -> Result<(), PluginError>;
}
