// === PUBLIC API (from SDK) ===
pub use oagw_sdk::{
    error::OagwError,
    models::{
        CreateRouteRequest, CreateUpstreamRequest, Endpoint, Route, RouteResponse,
        UpdateRouteRequest, UpdateUpstreamRequest, Upstream, UpstreamResponse,
    },
    service::{ControlPlaneService, DataPlaneService},
};

// === MODULE DEFINITION ===
pub mod module;
pub use module::OutboundApiGatewayModule;

// === INTERNAL MODULES ===
#[doc(hidden)]
pub mod api;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod client;
pub(crate) mod domain;
pub(crate) mod dp;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;
