//! Test utilities for CP integration tests.

use std::sync::Arc;

use modkit::client_hub::ClientHub;
use oagw_sdk::credential::CredentialResolver;
use oagw_sdk::service::ControlPlaneService;

use super::repo::{InMemoryCredentialResolver, InMemoryRouteRepo, InMemoryUpstreamRepo};
use super::service::ControlPlaneServiceImpl;

/// Re-export for tests that need to set credentials after creation.
pub use super::repo::credential_repo::InMemoryCredentialResolver as TestCredentialResolver;

/// Builder for a fully-wired Control Plane test environment.
pub struct TestCpBuilder {
    credentials: Vec<(String, String)>,
}

impl TestCpBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            credentials: Vec::new(),
        }
    }

    /// Pre-load credentials into the credential resolver.
    #[must_use]
    pub fn with_credentials(mut self, creds: Vec<(String, String)>) -> Self {
        self.credentials = creds;
        self
    }

    /// Create repos, service, and credential resolver, register them in the
    /// provided `ClientHub`, and return the CP service trait object.
    pub fn build_and_register(self, hub: &ClientHub) -> Arc<dyn ControlPlaneService> {
        let upstream_repo = Arc::new(InMemoryUpstreamRepo::new());
        let route_repo = Arc::new(InMemoryRouteRepo::new());
        let cp: Arc<dyn ControlPlaneService> =
            Arc::new(ControlPlaneServiceImpl::new(upstream_repo, route_repo));

        let cred_resolver: Arc<dyn CredentialResolver> = Arc::new(
            InMemoryCredentialResolver::with_credentials(self.credentials),
        );

        hub.register::<dyn ControlPlaneService>(cp.clone());
        hub.register::<dyn CredentialResolver>(cred_resolver);

        cp
    }
}

impl Default for TestCpBuilder {
    fn default() -> Self {
        Self::new()
    }
}
