// === PUBLIC API (from SDK) ===
pub use oagw_sdk::{
    error::OagwError,
    models::{
        CreateRouteRequest, CreateUpstreamRequest, Endpoint, Route, Upstream,
        UpdateRouteRequest, UpdateUpstreamRequest, UpstreamResponse, RouteResponse,
    },
    service::{ControlPlaneService, DataPlaneService},
};

// === MODULE DEFINITION ===
pub mod module;
pub use module::OagwModule;

// Force linkage of sub-module crates so their inventory registrations are included.
pub use oagw_cp::OagwCpModule;
pub use oagw_dp::OagwDpModule;

// === INTERNAL MODULES ===
#[doc(hidden)]
pub mod api;
#[doc(hidden)]
pub mod config;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_support;
