use axum::Json;
use axum::extract::{Extension, Path, Query};
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use oagw_sdk::gts;
use oagw_sdk::models::{CreateRouteRequest, Route, RouteResponse, UpdateRouteRequest};

use crate::api::rest::error::error_response;
use crate::api::rest::extractors::{PaginationQuery, TenantId, parse_gts_id};
use crate::module::AppState;

fn to_response(r: Route) -> RouteResponse {
    RouteResponse {
        id: gts::format_route_gts(r.id),
        tenant_id: r.tenant_id,
        upstream_id: r.upstream_id,
        match_rules: r.match_rules,
        plugins: r.plugins,
        rate_limit: r.rate_limit,
        tags: r.tags,
        priority: r.priority,
        enabled: r.enabled,
    }
}

pub async fn create_route(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Json(req): Json<CreateRouteRequest>,
) -> Result<impl IntoResponse, Response> {
    let route = state
        .cp
        .create_route(tenant.0, req)
        .await
        .map_err(error_response)?;
    Ok((StatusCode::CREATED, Json(to_response(route))))
}

pub async fn get_route(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/api/oagw/v1/routes/{id}"))?;
    let route = state
        .cp
        .get_route(tenant.0, uuid)
        .await
        .map_err(error_response)?;
    Ok(Json(to_response(route)))
}

pub async fn list_routes(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(upstream_id): Path<String>,
    Query(pagination): Query<PaginationQuery>,
) -> Result<impl IntoResponse, Response> {
    let upstream_uuid = parse_gts_id(
        &upstream_id,
        &format!("/api/oagw/v1/upstreams/{upstream_id}/routes"),
    )?;
    let query = pagination.to_list_query();
    let routes = state
        .cp
        .list_routes(tenant.0, upstream_uuid, &query)
        .await
        .map_err(error_response)?;
    let response: Vec<RouteResponse> = routes.into_iter().map(to_response).collect();
    Ok(Json(response))
}

pub async fn update_route(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
    Json(req): Json<UpdateRouteRequest>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/api/oagw/v1/routes/{id}"))?;
    let route = state
        .cp
        .update_route(tenant.0, uuid, req)
        .await
        .map_err(error_response)?;
    Ok(Json(to_response(route)))
}

pub async fn delete_route(
    Extension(state): Extension<AppState>,
    tenant: TenantId,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, Response> {
    let uuid = parse_gts_id(&id, &format!("/api/oagw/v1/routes/{id}"))?;
    state
        .cp
        .delete_route(tenant.0, uuid)
        .await
        .map_err(error_response)?;
    Ok(StatusCode::NO_CONTENT)
}
