use std::sync::Arc;

use async_trait::async_trait;
use modkit::api::OpenApiRegistry;
use modkit::{Module, ModuleCtx, RestApiCapability};
use oagw_sdk::service::{ControlPlaneService, DataPlaneService};
use tracing::info;

use crate::api::rest::routes;

/// Shared application state injected into all handlers.
#[derive(Clone)]
pub struct AppState {
    pub cp: Arc<dyn ControlPlaneService>,
    pub dp: Arc<dyn DataPlaneService>,
}

/// OAGW module: wires repos, services, and routes.
#[modkit::module(
    name = "oagw",
    deps = ["oagw-cp", "oagw-dp"],
    capabilities = [rest]
)]
pub struct OagwModule {
    state: arc_swap::ArcSwapOption<AppState>,
}

impl Default for OagwModule {
    fn default() -> Self {
        Self {
            state: arc_swap::ArcSwapOption::from(None),
        }
    }
}

#[async_trait]
impl Module for OagwModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        info!("Initializing OAGW module");

        let cp = ctx.client_hub().get::<dyn ControlPlaneService>()?;
        let dp = ctx.client_hub().get::<dyn DataPlaneService>()?;

        let app_state = AppState { cp, dp };

        self.state.store(Some(Arc::new(app_state)));
        info!("OAGW module initialized");
        Ok(())
    }
}

impl RestApiCapability for OagwModule {
    fn register_rest(
        &self,
        _ctx: &ModuleCtx,
        router: axum::Router,
        openapi: &dyn OpenApiRegistry,
    ) -> anyhow::Result<axum::Router> {
        info!("Registering OAGW REST routes");

        let state = self
            .state
            .load()
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("OAGW module not initialized — call init() first"))?
            .as_ref()
            .clone();

        let router = routes::register_routes(router, openapi, state);
        Ok(router)
    }
}
