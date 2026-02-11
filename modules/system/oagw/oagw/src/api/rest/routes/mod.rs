use axum::Router;
use modkit::api::OpenApiRegistry;

use crate::module::AppState;

mod proxy;
mod route;
mod upstream;

/// Register all OAGW REST routes with OpenAPI metadata.
pub fn register_routes(
    mut router: Router,
    openapi: &dyn OpenApiRegistry,
    state: AppState,
) -> Router {
    router = upstream::register(router, openapi);
    router = route::register(router, openapi);
    router = proxy::register(router);
    router.layer(axum::Extension(state))
}

/// Create a test router with all OAGW routes registered.
///
/// Uses manual route registration without OpenAPI metadata.
/// Suitable for integration tests that don't need an `OpenApiRegistry`.
#[cfg(any(test, feature = "test-utils"))]
pub fn test_router(state: AppState) -> Router {
    use axum::routing::{any, get, post};
    use crate::api::rest::handlers::{proxy as proxy_h, route as route_h, upstream as upstream_h};

    Router::new()
        // Upstream CRUD
        .route("/api/oagw/v1/upstreams", post(upstream_h::create_upstream))
        .route("/api/oagw/v1/upstreams", get(upstream_h::list_upstreams))
        .route(
            "/api/oagw/v1/upstreams/{id}",
            get(upstream_h::get_upstream)
                .put(upstream_h::update_upstream)
                .delete(upstream_h::delete_upstream),
        )
        // Route CRUD
        .route("/api/oagw/v1/routes", post(route_h::create_route))
        .route(
            "/api/oagw/v1/routes/{id}",
            get(route_h::get_route)
                .put(route_h::update_route)
                .delete(route_h::delete_route),
        )
        .route(
            "/api/oagw/v1/upstreams/{upstream_id}/routes",
            get(route_h::list_routes),
        )
        // Proxy
        .route("/api/oagw/v1/proxy/{*path}", any(proxy_h::proxy_handler))
        .layer(axum::Extension(state))
}
