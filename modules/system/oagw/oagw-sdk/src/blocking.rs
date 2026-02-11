//! Blocking API for OAGW client
//!
//! Provides synchronous wrappers for use in build scripts and other
//! non-async contexts.

use crate::error::ClientError;
use crate::types::{Request, Response};
use crate::OagwClient;

/// Blocking OAGW client API
///
/// This is a synchronous wrapper around the async OagwClient,
/// suitable for use in build scripts and other non-async contexts.
///
/// # Examples
/// ```ignore
/// // In build.rs
/// use oagw_sdk::{OagwClient, OagwClientConfig, Request, Method};
///
/// fn main() {
///     let config = OagwClientConfig::from_env().unwrap();
///     let client = OagwClient::from_config(config).unwrap();
///
///     let request = Request::builder()
///         .method(Method::GET)
///         .path("/elements@9.0.15/web-components.min.js")
///         .build()
///         .unwrap();
///
///     let response = client.blocking().execute("unpkg", request).unwrap();
///     let bytes = response.bytes_blocking().unwrap();
///
///     std::fs::write("assets/web-components.min.js", bytes).unwrap();
/// }
/// ```
pub struct BlockingClient<'a> {
    inner: &'a OagwClient,
}

impl<'a> BlockingClient<'a> {
    /// Create a new blocking client
    #[must_use]
    pub(crate) const fn new(inner: &'a OagwClient) -> Self {
        Self { inner }
    }

    /// Execute a request synchronously
    ///
    /// # Arguments
    /// * `alias` - Service alias (e.g., "openai", "unpkg")
    /// * `request` - Request to execute
    ///
    /// # Errors
    /// Returns error if:
    /// - Request fails
    /// - Runtime creation fails
    pub fn execute(&self, alias: &str, request: Request) -> Result<Response, ClientError> {
        // Try to use existing runtime if available
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                // Use existing runtime
                handle.block_on(self.inner.execute(alias, request))
            }
            Err(_) => {
                // Create temporary runtime
                let runtime = tokio::runtime::Runtime::new().map_err(|e| {
                    ClientError::Config(format!("Failed to create tokio runtime: {e}"))
                })?;
                runtime.block_on(self.inner.execute(alias, request))
            }
        }
    }
}

impl Response {
    /// Consume the response and return the body as bytes (blocking)
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - Runtime creation fails
    pub fn bytes_blocking(self) -> Result<bytes::Bytes, ClientError> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(self.bytes()),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new().map_err(|e| {
                    ClientError::Config(format!("Failed to create tokio runtime: {e}"))
                })?;
                runtime.block_on(self.bytes())
            }
        }
    }

    /// Consume the response and return the body as text (blocking)
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - Body is not valid UTF-8
    /// - Runtime creation fails
    pub fn text_blocking(self) -> Result<String, ClientError> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(self.text()),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new().map_err(|e| {
                    ClientError::Config(format!("Failed to create tokio runtime: {e}"))
                })?;
                runtime.block_on(self.text())
            }
        }
    }

    /// Consume the response and deserialize the body as JSON (blocking)
    ///
    /// # Errors
    /// Returns error if:
    /// - Stream reading fails
    /// - Body is not valid JSON
    /// - Runtime creation fails
    pub fn json_blocking<T: serde::de::DeserializeOwned>(self) -> Result<T, ClientError> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle.block_on(self.json()),
            Err(_) => {
                let runtime = tokio::runtime::Runtime::new().map_err(|e| {
                    ClientError::Config(format!("Failed to create tokio runtime: {e}"))
                })?;
                runtime.block_on(self.json())
            }
        }
    }
}
