use std::sync::Arc;

use async_trait::async_trait;
use modkit::context::ModuleCtx;
use modkit::Module;
use oagw_sdk::credential::CredentialResolver;
use oagw_sdk::service::{ControlPlaneService, DataPlaneService};
use tracing::info;

use crate::proxy::DataPlaneServiceImpl;

/// Data Plane module: proxy orchestration and plugin execution.
#[modkit::module(name = "oagw-dp", deps = ["oagw-cp"], capabilities = [])]
pub struct OagwDpModule {
    state: arc_swap::ArcSwapOption<Arc<dyn DataPlaneService>>,
}

impl Default for OagwDpModule {
    fn default() -> Self {
        Self {
            state: arc_swap::ArcSwapOption::from(None),
        }
    }
}

#[async_trait]
impl Module for OagwDpModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        info!("Initializing OAGW Data Plane module");

        let cp = ctx.client_hub().get::<dyn ControlPlaneService>()?;
        let cred_resolver = ctx.client_hub().get::<dyn CredentialResolver>()?;

        let dp: Arc<dyn DataPlaneService> =
            Arc::new(DataPlaneServiceImpl::new(cp, cred_resolver));

        ctx.client_hub()
            .register::<dyn DataPlaneService>(dp.clone());

        self.state.store(Some(Arc::new(dp)));
        info!("OAGW Data Plane module initialized");
        Ok(())
    }
}
