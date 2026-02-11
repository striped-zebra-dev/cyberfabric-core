//! Test utilities for cross-crate integration tests.
//!
//! Gated by `#[cfg(any(test, feature = "test-utils"))]`.

use std::sync::Arc;
use std::time::Duration;

use modkit::client_hub::ClientHub;
use oagw_sdk::credential::CredentialResolver;
use oagw_sdk::service::{ControlPlaneService, DataPlaneService};

use super::service::DataPlaneServiceImpl;

/// Re-export plugin ID constants for test configurations.
pub use super::plugin::apikey_auth::APIKEY_AUTH_PLUGIN_ID;

/// Builder for a fully-wired Data Plane test environment.
///
/// Requires that `dyn ControlPlaneService` and `dyn CredentialResolver`
/// are already registered in the `ClientHub` (e.g., via `TestCpBuilder`).
pub struct TestDpBuilder {
    request_timeout: Option<Duration>,
}

impl TestDpBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            request_timeout: None,
        }
    }

    /// Override the request timeout (useful for timeout tests).
    #[must_use]
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }

    /// Fetch CP and CredentialResolver from the hub, create a DP service,
    /// register it in `ClientHub`, and return the trait object.
    pub fn build_and_register(self, hub: &ClientHub) -> Arc<dyn DataPlaneService> {
        let cp = hub
            .get::<dyn ControlPlaneService>()
            .expect("ControlPlaneService must be registered before building DP");
        let cred_resolver = hub
            .get::<dyn CredentialResolver>()
            .expect("CredentialResolver must be registered before building DP");

        let mut svc = DataPlaneServiceImpl::new(cp, cred_resolver);
        if let Some(timeout) = self.request_timeout {
            svc = svc.with_request_timeout(timeout);
        }

        let dp: Arc<dyn DataPlaneService> = Arc::new(svc);
        hub.register::<dyn DataPlaneService>(dp.clone());

        dp
    }
}

impl Default for TestDpBuilder {
    fn default() -> Self {
        Self::new()
    }
}
