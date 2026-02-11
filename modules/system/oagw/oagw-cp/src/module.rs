use std::sync::Arc;

use async_trait::async_trait;
use modkit::context::ModuleCtx;
use modkit::Module;
use oagw_sdk::credential::CredentialResolver;
use oagw_sdk::service::ControlPlaneService;
use tracing::info;

use crate::domain::repo::{InMemoryCredentialResolver, InMemoryRouteRepo, InMemoryUpstreamRepo};
use crate::domain::service::ControlPlaneServiceImpl;

/// Control Plane module: manages upstreams, routes, and credential resolution.
#[modkit::module(name = "oagw-cp", capabilities = [])]
pub struct OagwCpModule {
    state: arc_swap::ArcSwapOption<Arc<dyn ControlPlaneService>>,
}

impl Default for OagwCpModule {
    fn default() -> Self {
        Self {
            state: arc_swap::ArcSwapOption::from(None),
        }
    }
}

#[async_trait]
impl Module for OagwCpModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        info!("Initializing OAGW Control Plane module");

        let upstream_repo = Arc::new(InMemoryUpstreamRepo::new());
        let route_repo = Arc::new(InMemoryRouteRepo::new());
        let cp: Arc<dyn ControlPlaneService> =
            Arc::new(ControlPlaneServiceImpl::new(upstream_repo, route_repo));

        let cred_resolver: Arc<dyn CredentialResolver> =
            Arc::new(InMemoryCredentialResolver::new());

        ctx.client_hub()
            .register::<dyn ControlPlaneService>(cp.clone());
        ctx.client_hub()
            .register::<dyn CredentialResolver>(cred_resolver);

        self.state.store(Some(Arc::new(cp)));
        info!("OAGW Control Plane module initialized");
        Ok(())
    }
}
