use std::sync::Arc;

use async_trait::async_trait;
use modkit::api::OpenApiRegistry;
use modkit::{Module, ModuleCtx, RestApiCapability};
use oagw_sdk::api::ServiceGatewayClientV1;
use oagw_sdk::client::OagwClientApi;
use crate::client::OagwClient;
use crate::domain::credential::CredentialResolver;
use tracing::info;

use crate::api::rest::routes;
use crate::domain::services::{
    ControlPlaneService, ControlPlaneServiceImpl, DataPlaneService, DataPlaneServiceImpl,
    ServiceGatewayClientV1Facade,
};
use crate::infra::storage::{InMemoryCredentialResolver, InMemoryRouteRepo, InMemoryUpstreamRepo};

/// Shared application state injected into all handlers.
#[derive(Clone)]
pub struct AppState {
    pub(crate) cp: Arc<dyn ControlPlaneService>,
    pub(crate) dp: Arc<dyn DataPlaneService>,
}

/// Outbound API Gateway module: wires repos, services, and routes.
#[modkit::module(
    name = "oagw",
    capabilities = [rest]
)]
pub struct OutboundApiGatewayModule {
    state: arc_swap::ArcSwapOption<AppState>,
}

impl Default for OutboundApiGatewayModule {
    fn default() -> Self {
        Self {
            state: arc_swap::ArcSwapOption::from(None),
        }
    }
}

#[async_trait]
impl Module for OutboundApiGatewayModule {
    async fn init(&self, ctx: &ModuleCtx) -> anyhow::Result<()> {
        info!("Initializing Outbound API Gateway module");

        // -- Control Plane init --
        let upstream_repo = Arc::new(InMemoryUpstreamRepo::new());
        let route_repo = Arc::new(InMemoryRouteRepo::new());
        let cp: Arc<dyn ControlPlaneService> =
            Arc::new(ControlPlaneServiceImpl::new(upstream_repo, route_repo));

        let cred_resolver: Arc<dyn CredentialResolver> =
            Arc::new(InMemoryCredentialResolver::new());

        ctx.client_hub()
            .register::<dyn CredentialResolver>(cred_resolver.clone());

        // -- Data Plane init --
        let dp: Arc<dyn DataPlaneService> =
            Arc::new(DataPlaneServiceImpl::new(cp.clone(), cred_resolver));

        // -- Facade (for external SDK consumers) --
        let oagw: Arc<dyn ServiceGatewayClientV1> =
            Arc::new(ServiceGatewayClientV1Facade::new(cp.clone(), dp.clone()));

        ctx.client_hub()
            .register::<dyn ServiceGatewayClientV1>(oagw.clone());

        

        let client: Arc<dyn OagwClientApi>  = Arc::new(OagwClient::from_ctx(ctx)?);
        ctx.client_hub().register::<dyn OagwClientApi>(client);

        let app_state = AppState { cp, dp };

        self.state.store(Some(Arc::new(app_state)));
        info!("Outbound API Gateway module initialized");
        Ok(())
    }
}

impl RestApiCapability for OutboundApiGatewayModule {
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
